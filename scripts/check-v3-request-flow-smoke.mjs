#!/usr/bin/env node

// MissionD V3 request-flow smoke checker.
//
// Cross-surface smoke gate for the user-facing MissionD path declared in
// .missiond/v3/missiond-blueprint.lisp. The checker is deterministic and
// read-only by default; --dry-fixture replaces the live repo reads with
// synthetic fixtures so it can run in CI without touching the workspace.
//
// What the checker pins:
//   1. The V3 blueprint's review-packet :state-derivation enumerates the six
//      states implemented by the request handler, with the six named rule
//      heads (plan-present-wins, plan-present-execute-requested,
//      plan-approved-event, intent-only-present, intent-drafting,
//      received-default).
//   2. The V3 blueprint's review-response :decisions list matches the six
//      decisions surfaced by mission_request respond.
//   3. The request handler surface (request.rs + request/review_packet.rs)
//      declares every wire state and decision string the blueprint promises.
//   4. A JS reimplementation of the state-derivation logic, run against
//      synthetic .missiond/requests/<request_id> directories, projects the
//      same six states the Rust handler would on identical inputs:
//        - request-only / no projection target -> received
//        - intent-alignment.lisp with :directive_id + :version
//          -> awaiting_intent_approval
//        - plan.lisp present, no approve_plan event
//          -> awaiting_plan_approval
//        - plan.lisp with :plan_id/:version/:board_task_id + an
//          approve_plan dispatched event -> awaiting_execution with
//          execute_plan in allowed_responses
//        - execute_plan dispatched event -> execute_requested with observe
//        - approve_plan event without plan.lisp, or plan.lisp missing the
//          materialized ref keys, produces a "missing-persisted-ref"
//          diagnostic so a malformed flow cannot silently advance.
//
// The default mode never dispatches a workstation task. An opt-in --live-ipc
// mode (wave43-01) calls the running MissionD daemon through mission_request
// over its tools/call IPC and drives the user-facing approval flow:
//   start -> respond approve_intent -> respond approve_plan -> stop.
// Live IPC stops at awaiting_execution and never calls execute_plan: the
// point is to prove the execution gate, not to consume a workstation slot.
// --confirm-execute is reserved as a future flag and is explicitly refused
// here — workstation dispatch is not the responsibility of this checker.
//
// Default live-IPC mode (wave44-01) is request-local-only: no write_file is
// passed to mission_request, so the daemon never writes the legacy
// .missiond/alignment/<topic>/intent-alignment.lisp or
// .missiond/plans/<plan_id>/PLAN.lisp compatibility artifacts. The smoke
// snapshots both compat roots before/after the live flow and fails if any
// new compat artifact appears that names this request_id or smoke objective.
// An optional --compat-write-file flag re-introduces compat_write_file=true
// to deliberately exercise the legacy opt-in path; that mode is never added
// to the v3 aggregate gate.
//
// Wave46-01 audit mode (--execute-dry-run): after approve_plan succeeds, the
// smoke calls mission_request respond with response=execute_plan, execute=true,
// dry_run=true, execute_mode=internal, dispatch_strategy=agent-team,
// target=mission_task_delegate. The smoke asserts the wave45 review-level
// invariants AND the workstation-dispatch substrate dry-run no-dispatch shape:
//   - respond_outcome=dispatched
//   - inner_action=unified_entry::plan_execute
//   - respond_result.execute=true
//   - review_packet.state=execute_requested
//   - allowed_responses=[observe]
//   - a request-local execute_plan review event was appended
//   - pipeline_result.execute_mode='internal'
//   - pipeline_result.status='dry_run'
//   - pipeline_result.runner_status='workstation_dispatch_v0'
//   - pipeline_result.workstation_dispatch_status='dry_run_no_dispatch'
//   - pipeline_result.target_tool='mission_task_delegate'
//   - pipeline_result.dispatch_strategy='agent-team'
//   - pipeline_result.task_brief_preview is a non-empty string
// Bridge mode (status=bridge_ready, runner_status=bridge_only) is no longer
// accepted as a no-dispatch proof for --execute-dry-run because it bypasses
// the workstation_dispatch substrate. The audit explicitly drives the
// substrate so MissionD reaches `run_workstation_dispatch_with_contract_and_trace`
// and emits `WorkstationDispatchOutcome::DryRun` instead of dispatching.
// The smoke never spawns or waits for a worker. Default --live-ipc still stops
// at awaiting_execution; only --execute-dry-run drives execute_plan.
//
// Wave49-01 audit mode (--restart-during-dispatch): PLAN-ONLY by default. After
// the wave47 --execute-real-dispatch step has produced a delegated BoardTask
// pinned to a dynamic slot, this opt-in mode emits a structured
// restart-recovery step plan that a parent/orchestrator can REVIEW and then
// execute manually against a live daemon. The script itself NEVER kills the
// daemon, never sends SIGTERM, and never invokes launchctl. The plan lists
// the exact parent-run command, the pre-restart observation steps, the
// expected pin-clear transition, and the re-dispatch assertion so the
// recovery contract from wave48-01 (autopilot.rs::dispatch_board_tasks
// clearing stale dynamic slot pins) can be live-verified end-to-end without
// surprising the operator.
// Validation: --restart-during-dispatch is REJECTED unless both --live-ipc
// and --execute-real-dispatch are also present, because the plan only makes
// sense once a real delegated BoardTask exists.
//
// Wave47-01 audit mode (--execute-real-dispatch): SLOW + SIDE-EFFECTING. After
// approve_plan succeeds, the smoke calls mission_request respond with
// response=execute_plan, execute=true, dry_run=false, execute_mode=internal,
// dispatch_strategy=agent-team, target=mission_task_delegate, cwd=<repo>, and
// a deliberately read-only smoke objective that tells the delegated worker to
// do no file edits and no commits. The substrate
// (run_workstation_dispatch_with_contract_and_trace) takes the
// WorkstationDispatchOutcome::Dispatched branch and creates a delegated
// BoardTask via mission_task_delegate. The smoke asserts the wave45/46
// review-level invariants AND the substrate dispatch shape:
//   - pipeline_result.status='executing'
//   - pipeline_result.execute_mode='internal'
//   - pipeline_result.runner_status='workstation_dispatch_v0'
//   - pipeline_result.workstation_dispatch_status='dispatched'
//   - pipeline_result.target_tool='mission_task_delegate'
//   - pipeline_result.dispatch_strategy='agent-team'
//   - pipeline_result.task_brief_preview is a non-empty string
//   - pipeline_result.inner_result is a non-null object
//   - pipeline_result.delegated_board_task_id is the delegated BoardTask UUID
// The smoke NEVER waits synchronously for the worker; the BoardTask stays in
// the queue for Autopilot to pick up. --cleanup removes only the request-local
// directory; the BoardTask + DB rows + worker-side artifacts remain. NEVER
// added to the v3 aggregate gate.
//
// CLI: node scripts/check-v3-request-flow-smoke.mjs [--json] [--dry-fixture]
//        [--blueprint <path>] [--repo <path>]
//        [--live-ipc [--endpoint <socket>] [--session-id <id>]
//                    [--request-id <id>] [--cleanup] [--compat-write-file]
//                    [--execute-dry-run] [--execute-real-dispatch]
//                    [--restart-during-dispatch] [--confirm-execute]]

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  head,
  isList,
  nodeText,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';
import { callToolViaIpc } from './task-runner-submit-dispatch.mjs';

const BLUEPRINT_PATH = '.missiond/v3/missiond-blueprint.lisp';
const REQUEST_HANDLER_PATHS = [
  'crates/missiond-daemon/src/handlers/knowledge/request.rs',
  'crates/missiond-daemon/src/handlers/knowledge/request/request_artifacts.rs',
  'crates/missiond-daemon/src/handlers/knowledge/request/review_packet.rs',
];
const MCP_REQUEST_PATH = 'crates/missiond-mcp/src/tools/knowledge/request.rs';

export const EXPECTED_STATES = [
  'received',
  'intent_drafting',
  'awaiting_intent_approval',
  'awaiting_plan_approval',
  'awaiting_execution',
  'execute_requested',
];

export const EXPECTED_RULE_HEADS = [
  'plan-present-wins',
  'plan-present-execute-requested',
  'plan-approved-event',
  'intent-only-present',
  'intent-drafting',
  'received-default',
];

export const EXPECTED_DECISIONS = [
  'approve_intent',
  'reject_intent',
  'ask_question',
  'approve_plan',
  'reject_plan',
  'execute_plan',
];

const usage = `Usage:
  node scripts/check-v3-request-flow-smoke.mjs [--json] [--dry-fixture]
    [--blueprint <path>] [--repo <path>]
    [--live-ipc [--endpoint <socket>] [--session-id <id>]
                [--request-id <id>] [--cleanup] [--confirm-execute]]

Cross-surface V3 request-flow smoke. By default, validates the V3 blueprint
review-packet/review-response contract, pins the wire states/decisions in
crates/missiond-daemon/src/handlers/knowledge/request.rs and the MCP schema,
and runs in-process fixture cases that drive the state classifier through
received -> awaiting_intent_approval -> awaiting_plan_approval ->
awaiting_execution -> execute_requested plus a malformed-ref diagnostic.

Flags:
  --json            Emit a structured JSON result (still exit 0/1 on ok/fail).
  --dry-fixture     Run only the fixture cases against synthetic
                    .missiond/requests/<request_id> directories; no live repo
                    reads. Useful for CI hygiene checks.
  --blueprint <p>   Override the V3 blueprint path (default ${BLUEPRINT_PATH}).
  --repo <p>        Override the repo root (default $PWD).
  --live-ipc        Opt-in: call a running MissionD daemon through
                    mission_request tools/call IPC and drive
                      start -> respond approve_intent -> respond approve_plan
                    , stopping at awaiting_execution. Asserts that real
                    request-local intent-alignment.lisp and plan.lisp
                    artifacts are produced and that plan.lisp gets stamped
                    with :plan_id / :version / :board_task_id by approve_plan.
                    Never calls execute_plan; never consumes a workstation
                    slot. Default static + fixture verification still runs.
  --endpoint <p>    UNIX socket (or host:port) for the daemon. Defaults to
                    \$MISSION_IPC_ENDPOINT, then \$MISSION_IPC_SOCKET, then
                    \$HOME/.missiond/missiond.sock.
  --session-id <id> Session id sent in tools/call _meta. Defaults to
                    \$CLAUDE_SESSION_ID / \$SESSION_ID / a wave43-prefixed
                    process-local id.
  --request-id <id> Request id used for the live flow. Auto-generated with a
                    wave43-live-ipc-smoke- prefix when omitted.
  --cleanup         After successful validation, remove only
                    .missiond/requests/<request_id>/ from disk. DB rows
                    (directives, plans, board_tasks) created by the live
                    flow remain as audit records. Cleanup never touches the
                    legacy compat roots .missiond/alignment/ or
                    .missiond/plans/.
  --compat-write-file
                    Opt into the legacy compatibility-writer path: passes
                    compat_write_file=true on start AND approve_intent so
                    the daemon writes .missiond/alignment/<topic>/intent-
                    alignment.lisp and .missiond/plans/<plan_id>/PLAN.lisp.
                    The smoke reports the compat artifacts in a separate
                    legacy_compat_artifacts block and does NOT count their
                    presence as a failure (the user explicitly asked for
                    them). Without this flag the default smoke FAILS if
                    any compat artifact appears that names this request.
  --execute-dry-run Opt into the wave46 audit mode. After approve_plan
                    succeeds, the smoke calls mission_request respond with
                    response=execute_plan, execute=true, dry_run=true,
                    execute_mode=internal, dispatch_strategy=agent-team,
                    target=mission_task_delegate. It asserts the wave45
                    review-level invariants (respond_outcome=dispatched,
                    inner_action=unified_entry::plan_execute,
                    respond_result.execute=true,
                    review_packet.state=execute_requested,
                    allowed_responses=[observe], request-local execute_plan
                    event appended) AND the wave46 workstation-dispatch
                    substrate dry-run shape (pipeline_result.execute_mode=
                    internal, status=dry_run, runner_status=
                    workstation_dispatch_v0, workstation_dispatch_status=
                    dry_run_no_dispatch, target_tool=mission_task_delegate,
                    dispatch_strategy=agent-team, task_brief_preview present).
                    Bridge mode is no longer accepted: the audit MUST drive
                    the workstation_dispatch substrate. The smoke never
                    spawns or waits for a worker. Without this flag
                    --live-ipc still stops at awaiting_execution.
  --restart-during-dispatch
                    Wave49 opt-in restart-recovery PLAN audit. Requires
                    --live-ipc AND --execute-real-dispatch (the planner
                    refuses unsafe combinations). After the real-dispatch
                    step produces a delegated BoardTask pinned to a dynamic
                    slot, the smoke emits a structured restart_recovery_plan
                    onto the live-ipc summary describing: (1) pre-restart
                    state capture, (2) the exact parent-run daemon-restart
                    command, (3) expected pin-clear transition, (4) expected
                    re-dispatch to a fresh slot, (5) recovery assertion
                    against wave48-01 autopilot.rs::dispatch_board_tasks
                    clear-stale-dyn-pin contract. The smoke NEVER kills the
                    daemon itself; the actual restart is parent-driven so
                    the operator can review the plan first. Without this
                    flag --execute-real-dispatch behavior is unchanged.
  --execute-real-dispatch
                    Wave47 opt-in REAL dispatch audit. SLOW + SIDE-EFFECTING.
                    After approve_plan succeeds, the smoke calls
                    mission_request respond with response=execute_plan,
                    execute=true, dry_run=false, execute_mode=internal,
                    dispatch_strategy=agent-team, target=mission_task_delegate,
                    cwd=<repo>, and a deliberately read-only smoke objective
                    that tells the delegated worker to do no file edits and
                    no commits. The substrate (run_workstation_dispatch_with_
                    contract_and_trace) takes the
                    WorkstationDispatchOutcome::Dispatched branch and creates
                    a delegated BoardTask via mission_task_delegate. The
                    smoke asserts pipeline_result.status='executing',
                    runner_status='workstation_dispatch_v0',
                    workstation_dispatch_status='dispatched',
                    target_tool='mission_task_delegate',
                    dispatch_strategy='agent-team', task_brief_preview is a
                    non-empty string, inner_result is a non-null object, and
                    delegated_board_task_id is a UUID. The smoke NEVER waits
                    synchronously for the worker; the BoardTask stays in the
                    queue for Autopilot. --cleanup removes only the
                    request-local directory; the BoardTask + DB rows + any
                    worker-side artifacts remain. NEVER appears in the
                    aggregate v3 gate.
  --confirm-execute Reserved for future use. This checker explicitly refuses
                    workstation dispatch; with --confirm-execute the run
                    still stops at awaiting_execution and prints a notice
                    pointing the user to --execute-real-dispatch instead.
                    Note that --execute-dry-run is the wave46-supported
                    no-slot audit flag and --execute-real-dispatch is the
                    wave47 opt-in real-dispatch flag; --confirm-execute
                    remains a no-op compat slot.
`;

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    json: false,
    dryFixture: false,
    liveIpc: false,
    confirmExecute: false,
    cleanup: false,
    compatWriteFile: false,
    executeDryRun: false,
    executeRealDispatch: false,
    restartDuringDispatch: false,
    endpoint: null,
    sessionId: null,
    requestId: null,
    blueprint: BLUEPRINT_PATH,
    repo: process.cwd(),
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--live-ipc') {
      opts.liveIpc = true;
    } else if (arg === '--confirm-execute') {
      opts.confirmExecute = true;
    } else if (arg === '--cleanup') {
      opts.cleanup = true;
    } else if (arg === '--compat-write-file') {
      opts.compatWriteFile = true;
    } else if (arg === '--execute-dry-run') {
      opts.executeDryRun = true;
    } else if (arg === '--execute-real-dispatch') {
      opts.executeRealDispatch = true;
    } else if (arg === '--restart-during-dispatch') {
      opts.restartDuringDispatch = true;
    } else if (arg === '--endpoint') {
      opts.endpoint = argv[++i] ?? fail('--endpoint requires a value');
    } else if (arg.startsWith('--endpoint=')) {
      opts.endpoint = arg.slice('--endpoint='.length);
    } else if (arg === '--session-id') {
      opts.sessionId = argv[++i] ?? fail('--session-id requires a value');
    } else if (arg.startsWith('--session-id=')) {
      opts.sessionId = arg.slice('--session-id='.length);
    } else if (arg === '--request-id') {
      opts.requestId = argv[++i] ?? fail('--request-id requires a value');
    } else if (arg.startsWith('--request-id=')) {
      opts.requestId = arg.slice('--request-id='.length);
    } else if (arg === '--blueprint') {
      opts.blueprint = argv[++i] ?? fail('--blueprint requires a value');
    } else if (arg.startsWith('--blueprint=')) {
      opts.blueprint = arg.slice('--blueprint='.length);
    } else if (arg === '--repo') {
      opts.repo = argv[++i] ?? fail('--repo requires a value');
    } else if (arg.startsWith('--repo=')) {
      opts.repo = arg.slice('--repo='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

// wave49-01: validate flag combinations before any IO. The restart-recovery
// plan only makes sense once the live flow has produced a real delegated
// BoardTask, so --restart-during-dispatch must accompany --live-ipc and
// --execute-real-dispatch. Returns { ok, errors } so unit/dry fixtures can
// assert the rejections without going through process.exit.
export function validateOpts(opts) {
  const errors = [];
  if (opts.restartDuringDispatch) {
    if (!opts.liveIpc) {
      errors.push('--restart-during-dispatch requires --live-ipc; the recovery plan operates against a running daemon.');
    }
    if (!opts.executeRealDispatch) {
      errors.push('--restart-during-dispatch requires --execute-real-dispatch; without a real delegated BoardTask there is nothing to recover.');
    }
  }
  return { ok: errors.length === 0, errors };
}

// wave49-01: pure planner for the restart-recovery step sequence. Pre-restart
// observations come from the realStep result captured by runLiveIpcSmoke;
// the actual daemon restart is intentionally documented as a parent-run
// shell command rather than executed here, so the operator gets a chance to
// review before touching the supervisor. The :assertion step encodes the
// wave48-01 contract (autopilot.rs::dispatch_board_tasks clears stale
// dynamic pins so the BoardTask re-routes to a fresh idle coder slot).
export function buildRestartRecoveryPlan({
  delegatedBoardTaskId,
  preRestartAssignee,
  requestId,
  repoRoot,
} = {}) {
  const taskRef = delegatedBoardTaskId ?? '<delegated_board_task_id>';
  const slotRef = preRestartAssignee ?? '<pre_restart_assignee>';
  return {
    schema: 'missiond.request-flow.restart-recovery-plan.v0',
    requires_flags: ['--live-ipc', '--execute-real-dispatch', '--restart-during-dispatch'],
    live_executed: false,
    live_execution_owner: 'parent/orchestrator (operator must run the restart command after reviewing this plan)',
    pre_restart_state: {
      delegated_board_task_id: delegatedBoardTaskId ?? null,
      pre_restart_assignee: preRestartAssignee ?? null,
      request_id: requestId ?? null,
      repo_root: repoRoot ?? null,
    },
    parent_run_command: {
      description: 'Daemon restart that wipes all active dynamic slots (main.rs:252-269 Phase 6.7). Pick whichever path matches your supervisor; both are safe under launchctl-managed missiond.',
      preferred:
        'launchctl kickstart -k gui/$(id -u)/com.missiond.daemon  # graceful kill + supervised restart',
      fallback:
        'pid=$(pgrep -f \"^/.+/missiond( |$)\" | head -1); test -n \"$pid\" && kill -TERM \"$pid\"  # macOS-compatible SIGTERM fallback; launchd brings it back within a few seconds',
    },
    expected_post_restart: {
      list_dynamic_slots_active_excludes: slotRef,
      board_task_assignee_transitions: ['Some(' + slotRef + ')', 'None', 'Some(<fresh_slot_id>)'],
      board_task_status_progression: ['open', 'running'],
      autopilot_note_pattern: '🔄 Pinned slot ' + slotRef + ' 在重启后已不可用，已解除 pin 等待重新调度',
    },
    steps: [
      {
        step: 1,
        name: 'capture_pre_restart_state',
        kind: 'observation',
        action:
          'Read pipeline_result from the preceding execute_plan_real_dispatch step. Record delegated_board_task_id and delegated_board_task_assignee. Optionally call mission_compute_slot list status=active to confirm the dynamic slot is currently registered in SlotManager.',
        expectation:
          'delegated_board_task_assignee matches a slot-dyn-* id and that slot appears in mission.list_slots() output.',
      },
      {
        step: 2,
        name: 'restart_daemon',
        kind: 'side_effect',
        live_only: true,
        action:
          'Run the parent_run_command above. Wait until mission_health/list_pages call (or any tools/call ping) succeeds again — typically 3-10 seconds under launchctl supervision.',
        expectation:
          'New missiond pid; main.rs:252-269 logs "Terminated active dynamic slots on startup (clean slate)" so the dynamic slot list is empty for ' + slotRef + '.',
      },
      {
        step: 3,
        name: 'observe_pin_clear',
        kind: 'observation',
        action:
          'Poll mission_board_get(taskId=' + taskRef + ') every 5 seconds for up to 5 minutes (Autopilot ticks every ~60s).',
        expectation:
          'BoardTask.assignee transitions Some(' + slotRef + ') -> None within 1-2 autopilot ticks. A board note matching autopilot_note_pattern is appended.',
      },
      {
        step: 4,
        name: 'observe_redispatch',
        kind: 'observation',
        action:
          'Continue polling mission_board_get(taskId=' + taskRef + ').',
        expectation:
          'BoardTask.assignee transitions None -> Some(<fresh_slot_id>) where fresh_slot_id != ' + slotRef + ', BoardTask.status moves open -> running, and pipeline_result on the new slot reflects the same task_brief.',
      },
      {
        step: 5,
        name: 'assert_recovery_proof',
        kind: 'assertion',
        action:
          'Compare new assignee to pre_restart_assignee and capture the autopilot note that justified the pin clear.',
        expectation:
          'new_assignee != ' + slotRef + ', BoardTask reaches a non-failed terminal state (done) without manual intervention; this proves the wave48-01 clear-stale-dyn-pin contract holds end-to-end.',
      },
    ],
  };
}

function defaultIpcEndpoint() {
  if (process.env.MISSION_IPC_ENDPOINT) return process.env.MISSION_IPC_ENDPOINT;
  if (process.env.MISSION_IPC_SOCKET) return process.env.MISSION_IPC_SOCKET;
  return path.join(os.homedir(), '.missiond', 'missiond.sock');
}

function defaultLiveSessionId() {
  return (
    process.env.CLAUDE_SESSION_ID
    ?? process.env.SESSION_ID
    ?? `wave43-live-ipc-smoke-${process.pid}`
  );
}

function generateLiveRequestId() {
  // Deterministic enough for human audit, unique enough to avoid collisions
  // across rapid retries.
  const stamp = new Date().toISOString().replace(/[^0-9]/g, '').slice(0, 14);
  return `wave43-live-ipc-smoke-${stamp}-${process.pid}`;
}

// Snapshot direct subdirectory names of a path, sorted, []. Missing dir = [].
function snapshotSubdirs(root) {
  try {
    return fs
      .readdirSync(root, { withFileTypes: true })
      .filter((e) => e.isDirectory())
      .map((e) => e.name)
      .sort();
  } catch {
    return [];
  }
}

// Read the JSON payload out of an MCP tools/call response. The daemon emits
// ToolResult { content: [{ type: 'text', text: <json> }], is_error? }; we
// re-hydrate the inner object so callers can assert on review_packet etc.
function parseToolResultPayload(toolResult, label) {
  if (!toolResult || !Array.isArray(toolResult.content) || toolResult.content.length === 0) {
    return { ok: false, payload: null, error: `${label}: tool result has no content` };
  }
  const first = toolResult.content[0];
  const text = first?.text ?? '';
  let payload = null;
  try {
    payload = JSON.parse(text);
  } catch (err) {
    return { ok: false, payload: null, error: `${label}: tool result is not JSON: ${err.message}` };
  }
  return { ok: true, payload, isError: toolResult.is_error === true };
}

// ── Blueprint structural validation ────────────────────────────────────

function validateBlueprintAst(forms, file) {
  const diagnostics = [];
  const root = forms.find((f) => isList(f) && head(f) === 'missiond-blueprint');
  if (!root) {
    diagnostics.push({ file, message: 'no (missiond-blueprint ...) root form' });
    return { ok: false, diagnostics };
  }

  const unifiedEntry = root.children.find(
    (c) => isList(c) && head(c) === 'unified-entry',
  );
  if (!unifiedEntry) {
    diagnostics.push({ file, message: 'blueprint missing (unified-entry ...) section' });
    return { ok: false, diagnostics };
  }

  const reviewPacket = unifiedEntry.children.find(
    (c) => isList(c) && head(c) === 'review-packet',
  );
  if (!reviewPacket) {
    diagnostics.push({ file, message: 'unified-entry missing (review-packet ...)' });
  } else {
    const props = readKeywordProps(reviewPacket, { start: 1 });
    const statesNode = props[':states']?.value;
    const declaredStates = statesNode && isList(statesNode)
      ? statesNode.children
          .map((c) => nodeText(c))
          .filter((v) => v != null)
          .map((v) => v.replace(/^:/, ''))
      : [];
    for (const s of EXPECTED_STATES) {
      if (!declaredStates.includes(s)) {
        diagnostics.push({
          file,
          message: `review-packet :states must include "${s}"; declared = ${JSON.stringify(declaredStates)}`,
        });
      }
    }
    // Extract :state-derivation rule heads (each rule is `(rule <head> ...)`).
    const derivationNode = props[':state-derivation']?.value;
    const ruleHeads = derivationNode && isList(derivationNode)
      ? derivationNode.children
          .filter((c) => isList(c) && head(c) === 'rule')
          .map((c) => nodeText(c.children[1]))
          .filter((v) => v != null)
      : [];
    for (const r of EXPECTED_RULE_HEADS) {
      if (!ruleHeads.includes(r)) {
        diagnostics.push({
          file,
          message: `review-packet :state-derivation must include rule "${r}"; got ${JSON.stringify(ruleHeads)}`,
        });
      }
    }
    // Allowed-responses must cover both modes. The block is shaped
    // ((human-interactive ...) (trusted-agent ...))
    const allowedNode = props[':allowed-responses']?.value;
    const allowedModes = allowedNode && isList(allowedNode)
      ? allowedNode.children
          .filter((c) => isList(c))
          .map((c) => nodeText(c.children[0]))
          .filter((v) => v != null)
      : [];
    for (const m of ['human-interactive', 'trusted-agent']) {
      if (!allowedModes.includes(m)) {
        diagnostics.push({
          file,
          message: `review-packet :allowed-responses must declare mode "${m}"; got ${JSON.stringify(allowedModes)}`,
        });
      }
    }
  }

  const reviewResponse = unifiedEntry.children.find(
    (c) => isList(c) && head(c) === 'review-response',
  );
  if (!reviewResponse) {
    diagnostics.push({ file, message: 'unified-entry missing (review-response ...)' });
  } else {
    const props = readKeywordProps(reviewResponse, { start: 1 });
    const decisionsNode = props[':decisions']?.value;
    const declaredDecisions = decisionsNode && isList(decisionsNode)
      ? decisionsNode.children
          .map((c) => nodeText(c))
          .filter((v) => v != null)
      : [];
    for (const d of EXPECTED_DECISIONS) {
      if (!declaredDecisions.includes(d)) {
        diagnostics.push({
          file,
          message: `review-response :decisions must include "${d}"; declared = ${JSON.stringify(declaredDecisions)}`,
        });
      }
    }
  }

  return { ok: diagnostics.length === 0, diagnostics };
}

function validateRequestHandlerSource(source, file) {
  const diagnostics = [];
  // Pin the wire strings the handler emits so a rename in Rust is caught.
  const wireExpectations = [
    'enum ReviewState',
    'fn classify_review_state',
    'fn latest_review_event_checkpoint',
    'fn allowed_responses_for',
    'fn derive_review_packet',
    'fn build_review_event_lisp',
    'fn enrich_materialized_plan_lisp',
    'fn extract_plan_ref_from_artifact',
    'Self::Received => "received"',
    'Self::IntentDrafting => "intent_drafting"',
    'Self::AwaitingIntentApproval => "awaiting_intent_approval"',
    'Self::AwaitingPlanApproval => "awaiting_plan_approval"',
    'Self::AwaitingExecution => "awaiting_execution"',
    'Self::ExecuteRequested => "execute_requested"',
    ':decision :execute_plan',
    ':decision :approve_plan',
    ':outcome :dispatched',
    'execute_plan',
    'approve_plan',
    'observe',
  ];
  for (const needle of wireExpectations) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `request handler missing wire pin: ${needle}` });
    }
  }
  return { ok: diagnostics.length === 0, diagnostics };
}

function validateMcpRequestSource(source, file) {
  const diagnostics = [];
  for (const decision of EXPECTED_DECISIONS) {
    if (!source.includes(`"${decision}"`)) {
      diagnostics.push({
        file,
        message: `MCP mission_request schema must enumerate decision "${decision}"`,
      });
    }
  }
  for (const state of EXPECTED_STATES) {
    if (!source.includes(state)) {
      diagnostics.push({
        file,
        message: `MCP mission_request description must mention state "${state}"`,
      });
    }
  }
  return { ok: diagnostics.length === 0, diagnostics };
}

// ── JS port of the Rust state classifier (pure, no IO beyond fs reads
//    handed in by the caller). Mirrors classify_review_state +
//    latest_review_event_checkpoint + allowed_responses_for. ──────────

export function classifyReviewStateJs({
  existence,
  projectionTarget,
  executeRequested,
  reviewCheckpoint,
}) {
  let state;
  let artifactKind;
  if (existence.plan && (executeRequested || reviewCheckpoint === 'execute_requested')) {
    state = 'execute_requested';
    artifactKind = 'plan';
  } else if (existence.plan && reviewCheckpoint === 'plan_approved') {
    state = 'awaiting_execution';
    artifactKind = 'plan';
  } else if (existence.plan) {
    state = 'awaiting_plan_approval';
    artifactKind = 'plan';
  } else if (existence.intent_alignment) {
    state = 'awaiting_intent_approval';
    artifactKind = 'intent_alignment';
  } else if (projectionTarget) {
    state = 'intent_drafting';
    artifactKind = projectionTarget;
  } else {
    state = 'received';
    artifactKind = 'request';
  }
  return { state, artifactKind };
}

export function latestReviewEventCheckpointJs(eventTexts) {
  for (let i = eventTexts.length - 1; i >= 0; i -= 1) {
    const text = eventTexts[i];
    if (text.includes(':decision :execute_plan')) {
      if (text.includes(':outcome :dispatched')) return 'execute_requested';
      continue;
    }
    if (text.includes(':decision :approve_plan')) {
      if (text.includes(':outcome :dispatched')) return 'plan_approved';
      continue;
    }
    if (
      text.includes(':decision :reject_plan')
      || text.includes(':decision :ask_question')
      || text.includes(':decision :approve_intent')
      || text.includes(':decision :reject_intent')
    ) {
      return null;
    }
  }
  return null;
}

export function allowedResponsesForJs(mode, state) {
  if (mode === 'human_interactive') {
    if (state === 'awaiting_intent_approval') return ['approve_intent', 'reject_intent', 'ask_question'];
    if (state === 'awaiting_plan_approval') return ['approve_plan', 'reject_plan', 'ask_question'];
  }
  if (mode === 'trusted_agent') {
    if (state === 'awaiting_intent_approval') return ['approve_intent', 'ask_question'];
    if (state === 'awaiting_plan_approval') return ['approve_plan', 'ask_question'];
  }
  if (state === 'awaiting_execution') return ['execute_plan', 'ask_question'];
  if (state === 'execute_requested') return ['observe'];
  return ['observe'];
}

// Lightweight Lisp keyword extraction that matches the Rust
// extract_lisp_keyword_string semantics (text-based, tolerant of formatting).
export function extractLispKeywordString(text, key) {
  const needle = `:${key}`;
  let cursor = 0;
  while (true) {
    const idx = text.indexOf(needle, cursor);
    if (idx < 0) return null;
    let after = idx + needle.length;
    while (after < text.length && /[ \t\r\n]/.test(text[after])) after += 1;
    if (text[after] === '"') {
      const close = text.indexOf('"', after + 1);
      if (close > after + 1) {
        return text.slice(after + 1, close);
      }
    }
    cursor = idx + needle.length;
  }
}

function isUuidShaped(id) {
  return /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(id);
}

export function extractPlanRefFromArtifact(text) {
  const planId = extractLispKeywordString(text, 'plan_id');
  if (planId) return { id: planId };
  const id = extractLispKeywordString(text, 'id');
  if (id && isUuidShaped(id)) return { id };
  return null;
}

export function extractDirectiveRefFromArtifact(text) {
  let id = extractLispKeywordString(text, 'directive_id');
  if (!id) {
    const fallback = extractLispKeywordString(text, 'id');
    if (fallback && isUuidShaped(fallback)) id = fallback;
  }
  if (!id) return null;
  const versionMatch = text.match(/:directive_version\s+(\d+)/) || text.match(/:version\s+(\d+)/);
  if (!versionMatch) return null;
  return { id, version: Number.parseInt(versionMatch[1], 10) };
}

// Read a synthetic request directory and run the same packet derivation
// + ref-resolution checks the Rust handler performs at request boundaries.
function evaluateRequestDir(requestDir, { mode = 'human_interactive', executeRequested = false } = {}) {
  const requestPath = path.join(requestDir, 'request.lisp');
  const intentPath = path.join(requestDir, 'intent-alignment.lisp');
  const planPath = path.join(requestDir, 'plan.lisp');
  const eventsDir = path.join(requestDir, 'events');

  const existence = {
    request: fs.existsSync(requestPath),
    intent_alignment: fs.existsSync(intentPath),
    plan: fs.existsSync(planPath),
  };

  const eventTexts = fs.existsSync(eventsDir)
    ? fs
        .readdirSync(eventsDir)
        .filter((n) => n.endsWith('.event.lisp'))
        .sort()
        .map((n) => fs.readFileSync(path.join(eventsDir, n), 'utf8'))
    : [];
  const checkpoint = latestReviewEventCheckpointJs(eventTexts);

  const { state, artifactKind } = classifyReviewStateJs({
    existence,
    projectionTarget: null,
    executeRequested,
    reviewCheckpoint: checkpoint,
  });
  const allowed = allowedResponsesForJs(mode, state);

  const refDiagnostics = [];
  if (state === 'awaiting_intent_approval') {
    const text = fs.readFileSync(intentPath, 'utf8');
    const ref = extractDirectiveRefFromArtifact(text);
    if (!ref) {
      refDiagnostics.push('intent-alignment.lisp missing :directive_id + :version');
    }
  }
  if (state === 'awaiting_plan_approval' || state === 'awaiting_execution' || state === 'execute_requested') {
    const text = existence.plan ? fs.readFileSync(planPath, 'utf8') : '';
    if (!existence.plan) {
      refDiagnostics.push('plan.lisp missing while review-checkpoint expected one');
    } else if (state === 'awaiting_execution' || state === 'execute_requested') {
      const planRef = extractPlanRefFromArtifact(text);
      if (!planRef) {
        refDiagnostics.push('plan.lisp missing materialized :plan_id (must follow approve_plan)');
      }
      if (extractLispKeywordString(text, 'board_task_id') == null) {
        refDiagnostics.push('plan.lisp missing :board_task_id (materialization stamp incomplete)');
      }
      const versionMatch = text.match(/:version\s+(\d+)/);
      if (!versionMatch) {
        refDiagnostics.push('plan.lisp missing :version (materialization stamp incomplete)');
      }
    }
  }
  // A dispatched approve_plan event without plan.lisp is an inconsistent flow.
  if (checkpoint === 'plan_approved' && !existence.plan) {
    refDiagnostics.push('approve_plan event recorded but plan.lisp absent');
  }

  return { existence, eventCount: eventTexts.length, checkpoint, state, artifactKind, allowed, refDiagnostics };
}

// ── Fixture cases ──────────────────────────────────────────────────────

function writeFixtureFile(file, body) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, body);
}

function buildRequestLisp(rid, mode = 'human-interactive') {
  return `;; MissionD request artifact.
(mission-request "${rid}"
  :schema "missiond.request.v1"
  :request_id "${rid}"
  :mode :${mode}
  :objective "smoke-fixture"
  :state :received)\n`;
}

function buildIntentAlignmentLisp({ directiveId, version, withRef = true }) {
  if (!withRef) {
    return '(intent-alignment :schema "missiond.intent-alignment.v1")\n';
  }
  return `(intent-alignment
  :schema "missiond.intent-alignment.v1"
  :directive_id "${directiveId}"
  :directive_version ${version}
  :version ${version})\n`;
}

function buildPlanLisp({ planId, version, boardTaskId, withRef = true }) {
  if (!withRef) {
    return '(plan :schema "missiond.plan.v1" :nodes ((:id "root")))\n';
  }
  return `(plan
  :schema "missiond.plan.v1"
  :plan_id "${planId}"
  :version ${version}
  :board_task_id "${boardTaskId}"
  :nodes ((:id "root")))\n`;
}

function buildEventLisp({ seq, decision, outcome, planId, directiveId }) {
  let body = `(lifecycle-event "evt-fixture-${String(seq).padStart(6, '0')}"\n`;
  body += '  :schema "missiond.lifecycle-event.v1"\n';
  body += `  :seq ${seq}\n`;
  body += `  :kind :review_response_${outcome}\n`;
  body += '  :payload\n';
  body += `    (:decision :${decision}\n`;
  body += `     :outcome :${outcome}\n`;
  if (planId) body += `     :plan_id "${planId}"\n`;
  if (directiveId) body += `     :directive_id "${directiveId}"\n`;
  body += '    )\n';
  body += `  :idempotency_key "fx/${decision}/${seq}")\n`;
  return body;
}

function buildFixtureCases() {
  return [
    {
      name: 'received: only request.lisp present, no projection target',
      mode: 'human_interactive',
      executeRequested: false,
      writeFiles: ({ requestDir }) => {
        writeFixtureFile(path.join(requestDir, 'request.lisp'), buildRequestLisp('req-rcv'));
      },
      expect: {
        state: 'received',
        artifactKind: 'request',
        allowed: ['observe'],
        refDiagnostics: [],
      },
    },
    {
      name: 'awaiting_intent_approval: intent-alignment.lisp with :directive_id + :version',
      mode: 'human_interactive',
      executeRequested: false,
      writeFiles: ({ requestDir }) => {
        writeFixtureFile(path.join(requestDir, 'request.lisp'), buildRequestLisp('req-int'));
        writeFixtureFile(
          path.join(requestDir, 'intent-alignment.lisp'),
          buildIntentAlignmentLisp({
            directiveId: '11111111-2222-3333-4444-555555555555',
            version: 1,
          }),
        );
      },
      expect: {
        state: 'awaiting_intent_approval',
        artifactKind: 'intent_alignment',
        allowed: ['approve_intent', 'reject_intent', 'ask_question'],
        refDiagnostics: [],
      },
    },
    {
      name: 'awaiting_intent_approval but malformed: missing :directive_id + :version flagged',
      mode: 'human_interactive',
      executeRequested: false,
      writeFiles: ({ requestDir }) => {
        writeFixtureFile(path.join(requestDir, 'request.lisp'), buildRequestLisp('req-bad-int'));
        writeFixtureFile(
          path.join(requestDir, 'intent-alignment.lisp'),
          buildIntentAlignmentLisp({ withRef: false }),
        );
      },
      expect: {
        state: 'awaiting_intent_approval',
        artifactKind: 'intent_alignment',
        allowed: ['approve_intent', 'reject_intent', 'ask_question'],
        // The missing-ref diagnostic is what the Rust handler turns into a
        // structured blocked response when the user tries approve_intent.
        refDiagnosticsContain: 'intent-alignment.lisp missing :directive_id',
      },
    },
    {
      name: 'awaiting_plan_approval: plan.lisp present, no approve_plan event',
      mode: 'human_interactive',
      executeRequested: false,
      writeFiles: ({ requestDir }) => {
        writeFixtureFile(path.join(requestDir, 'request.lisp'), buildRequestLisp('req-plan'));
        writeFixtureFile(
          path.join(requestDir, 'plan.lisp'),
          buildPlanLisp({ withRef: false }),
        );
      },
      expect: {
        state: 'awaiting_plan_approval',
        artifactKind: 'plan',
        allowed: ['approve_plan', 'reject_plan', 'ask_question'],
        refDiagnostics: [],
      },
    },
    {
      name: 'awaiting_execution: plan.lisp materialized + approve_plan dispatched event',
      mode: 'human_interactive',
      executeRequested: false,
      writeFiles: ({ requestDir }) => {
        writeFixtureFile(path.join(requestDir, 'request.lisp'), buildRequestLisp('req-aex'));
        writeFixtureFile(
          path.join(requestDir, 'plan.lisp'),
          buildPlanLisp({
            planId: '99999999-8888-7777-6666-555555555555',
            version: 1,
            boardTaskId: 'btk-fixture',
          }),
        );
        writeFixtureFile(
          path.join(requestDir, 'events', '000002.event.lisp'),
          buildEventLisp({
            seq: 2,
            decision: 'approve_plan',
            outcome: 'dispatched',
            planId: '99999999-8888-7777-6666-555555555555',
          }),
        );
      },
      expect: {
        state: 'awaiting_execution',
        artifactKind: 'plan',
        allowed: ['execute_plan', 'ask_question'],
        refDiagnostics: [],
      },
    },
    {
      name: 'awaiting_execution malformed: approve_plan event without :plan_id stamp',
      mode: 'human_interactive',
      executeRequested: false,
      writeFiles: ({ requestDir }) => {
        writeFixtureFile(path.join(requestDir, 'request.lisp'), buildRequestLisp('req-bad-plan'));
        // plan.lisp exists but no :plan_id / :version / :board_task_id was
        // stamped — emulating a flow where approve_plan dispatched but the
        // post-approval write-back failed.
        writeFixtureFile(
          path.join(requestDir, 'plan.lisp'),
          buildPlanLisp({ withRef: false }),
        );
        writeFixtureFile(
          path.join(requestDir, 'events', '000002.event.lisp'),
          buildEventLisp({ seq: 2, decision: 'approve_plan', outcome: 'dispatched' }),
        );
      },
      expect: {
        state: 'awaiting_execution',
        artifactKind: 'plan',
        allowed: ['execute_plan', 'ask_question'],
        refDiagnosticsContain: 'plan.lisp missing materialized :plan_id',
      },
    },
    {
      name: 'execute_requested: execute_plan dispatched event yields observe',
      mode: 'human_interactive',
      executeRequested: false,
      writeFiles: ({ requestDir }) => {
        writeFixtureFile(path.join(requestDir, 'request.lisp'), buildRequestLisp('req-exec'));
        writeFixtureFile(
          path.join(requestDir, 'plan.lisp'),
          buildPlanLisp({
            planId: '00000000-1111-2222-3333-444444444444',
            version: 2,
            boardTaskId: 'btk-exec',
          }),
        );
        writeFixtureFile(
          path.join(requestDir, 'events', '000002.event.lisp'),
          buildEventLisp({
            seq: 2,
            decision: 'approve_plan',
            outcome: 'dispatched',
            planId: '00000000-1111-2222-3333-444444444444',
          }),
        );
        writeFixtureFile(
          path.join(requestDir, 'events', '000003.event.lisp'),
          buildEventLisp({
            seq: 3,
            decision: 'execute_plan',
            outcome: 'dispatched',
            planId: '00000000-1111-2222-3333-444444444444',
          }),
        );
      },
      expect: {
        state: 'execute_requested',
        artifactKind: 'plan',
        allowed: ['observe'],
        refDiagnostics: [],
      },
    },
    {
      name: 'execute=true short-circuit: plan present + execute_requested arg yields execute_requested',
      mode: 'trusted_agent',
      executeRequested: true,
      writeFiles: ({ requestDir }) => {
        writeFixtureFile(path.join(requestDir, 'request.lisp'), buildRequestLisp('req-exec-arg', 'trusted-agent'));
        writeFixtureFile(
          path.join(requestDir, 'plan.lisp'),
          buildPlanLisp({
            planId: '12345678-1234-1234-1234-123456789012',
            version: 1,
            boardTaskId: 'btk-trusted',
          }),
        );
      },
      expect: {
        state: 'execute_requested',
        artifactKind: 'plan',
        allowed: ['observe'],
        refDiagnostics: [],
      },
    },
    {
      name: 'malformed: approve_plan dispatched event without plan.lisp produces diagnostic',
      mode: 'human_interactive',
      executeRequested: false,
      writeFiles: ({ requestDir }) => {
        writeFixtureFile(path.join(requestDir, 'request.lisp'), buildRequestLisp('req-mal'));
        writeFixtureFile(
          path.join(requestDir, 'events', '000002.event.lisp'),
          buildEventLisp({ seq: 2, decision: 'approve_plan', outcome: 'dispatched' }),
        );
      },
      // No plan.lisp -> classifier falls back through to received because
      // none of the artifact files exist. The event-only state is not a
      // valid resting point and the diagnostic captures it.
      expect: {
        state: 'received',
        artifactKind: 'request',
        allowed: ['observe'],
        refDiagnosticsContain: 'approve_plan event recorded but plan.lisp absent',
      },
    },
  ];
}

// wave49-01 dry-fixture coverage for the --restart-during-dispatch parser
// gate and for the structural shape of the restart-recovery plan. Returns
// { totalCases, failedCases } so it can compose with runFixtures into a
// single fixture-summary line. Pure: never touches a daemon, never spawns,
// never writes outside the temp dir.
function runRestartRecoveryFixtures(diagnostics) {
  const cases = [
    {
      name: 'default opts validate ok (no restart flag)',
      opts: {
        liveIpc: false,
        executeRealDispatch: false,
        restartDuringDispatch: false,
      },
      expect: { ok: true, errorContains: null },
    },
    {
      name: 'live-ipc only without restart flag still validates ok',
      opts: {
        liveIpc: true,
        executeRealDispatch: false,
        restartDuringDispatch: false,
      },
      expect: { ok: true, errorContains: null },
    },
    {
      name: 'restart-during-dispatch alone is rejected',
      opts: {
        liveIpc: false,
        executeRealDispatch: false,
        restartDuringDispatch: true,
      },
      expect: { ok: false, errorContains: 'requires --live-ipc' },
    },
    {
      name: 'restart-during-dispatch + live-ipc without real-dispatch is rejected',
      opts: {
        liveIpc: true,
        executeRealDispatch: false,
        restartDuringDispatch: true,
      },
      expect: { ok: false, errorContains: 'requires --execute-real-dispatch' },
    },
    {
      name: 'restart-during-dispatch + execute-real-dispatch without live-ipc is rejected',
      opts: {
        liveIpc: false,
        executeRealDispatch: true,
        restartDuringDispatch: true,
      },
      expect: { ok: false, errorContains: 'requires --live-ipc' },
    },
    {
      name: 'restart-during-dispatch with both gates validates ok',
      opts: {
        liveIpc: true,
        executeRealDispatch: true,
        restartDuringDispatch: true,
      },
      expect: { ok: true, errorContains: null },
    },
  ];
  let failed = 0;
  for (const c of cases) {
    const result = validateOpts(c.opts);
    const fxFails = [];
    if (result.ok !== c.expect.ok) {
      fxFails.push(`expected ok=${c.expect.ok}, got ${result.ok} (errors=${JSON.stringify(result.errors)})`);
    }
    if (
      c.expect.errorContains
      && !result.errors.some((e) => e.includes(c.expect.errorContains))
    ) {
      fxFails.push(
        `expected an error containing "${c.expect.errorContains}", got ${JSON.stringify(result.errors)}`,
      );
    }
    if (fxFails.length > 0) {
      failed += 1;
      diagnostics.push({
        file: 'restart-recovery-fixture',
        message: `restart-recovery fixture FAILED [${c.name}]: ${fxFails.join('; ')}`,
      });
    }
  }

  // Plan structural fixture: with synthetic pre-restart inputs, the planner
  // must emit the five-step recovery sequence in order, name each step's
  // kind, and reproduce the slot/task identifiers in the parent_run_command
  // expectations. This gives the parent a stable contract to bind against.
  const plan = buildRestartRecoveryPlan({
    delegatedBoardTaskId: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
    preRestartAssignee: 'slot-dyn-fixture',
    requestId: 'wave49-fixture-request',
    repoRoot: '/tmp/fixture-repo',
  });
  const planFails = [];
  const expectedNames = [
    'capture_pre_restart_state',
    'restart_daemon',
    'observe_pin_clear',
    'observe_redispatch',
    'assert_recovery_proof',
  ];
  if (!Array.isArray(plan.steps) || plan.steps.length !== expectedNames.length) {
    planFails.push(
      `expected ${expectedNames.length} steps, got ${Array.isArray(plan.steps) ? plan.steps.length : typeof plan.steps}`,
    );
  } else {
    for (let i = 0; i < expectedNames.length; i += 1) {
      const observed = plan.steps[i];
      if (observed?.name !== expectedNames[i]) {
        planFails.push(
          `step ${i + 1} expected name=${expectedNames[i]}, got ${JSON.stringify(observed?.name)}`,
        );
      }
      if (observed?.step !== i + 1) {
        planFails.push(
          `step ${i + 1} expected step=${i + 1}, got ${JSON.stringify(observed?.step)}`,
        );
      }
      if (
        typeof observed?.action !== 'string'
        || observed.action.trim() === ''
      ) {
        planFails.push(`step ${expectedNames[i]} has empty action string`);
      }
      if (
        typeof observed?.expectation !== 'string'
        || observed.expectation.trim() === ''
      ) {
        planFails.push(`step ${expectedNames[i]} has empty expectation string`);
      }
    }
  }
  if (plan.live_executed !== false) {
    planFails.push('plan.live_executed must be false in this shard (live restart is parent-driven)');
  }
  if (
    !Array.isArray(plan.requires_flags)
    || !plan.requires_flags.includes('--restart-during-dispatch')
    || !plan.requires_flags.includes('--live-ipc')
    || !plan.requires_flags.includes('--execute-real-dispatch')
  ) {
    planFails.push(
      `plan.requires_flags missing one of [--live-ipc, --execute-real-dispatch, --restart-during-dispatch], got ${JSON.stringify(plan.requires_flags)}`,
    );
  }
  if (
    typeof plan.parent_run_command?.preferred !== 'string'
    || !plan.parent_run_command.preferred.includes('launchctl')
  ) {
    planFails.push(
      `plan.parent_run_command.preferred should reference launchctl; got ${JSON.stringify(plan.parent_run_command?.preferred)}`,
    );
  }
  if (
    typeof plan.parent_run_command?.fallback !== 'string'
    || !plan.parent_run_command.fallback.includes('pgrep')
    || plan.parent_run_command.fallback.includes('xargs -r')
  ) {
    planFails.push(
      `plan.parent_run_command.fallback should be macOS-compatible and avoid GNU xargs -r; got ${JSON.stringify(plan.parent_run_command?.fallback)}`,
    );
  }
  if (
    plan.pre_restart_state?.delegated_board_task_id
    !== 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'
    || plan.pre_restart_state?.pre_restart_assignee !== 'slot-dyn-fixture'
  ) {
    planFails.push('planner did not echo synthetic pre_restart_state inputs back into the plan');
  }
  if (
    plan.expected_post_restart?.list_dynamic_slots_active_excludes
    !== 'slot-dyn-fixture'
  ) {
    planFails.push(
      `plan.expected_post_restart.list_dynamic_slots_active_excludes should equal pre_restart_assignee; got ${JSON.stringify(plan.expected_post_restart?.list_dynamic_slots_active_excludes)}`,
    );
  }
  if (planFails.length > 0) {
    failed += 1;
    diagnostics.push({
      file: 'restart-recovery-fixture',
      message: `restart-recovery plan structural fixture FAILED: ${planFails.join('; ')}`,
    });
  }

  // +1 case for the plan structural fixture itself.
  return { totalCases: cases.length + 1, failedCases: failed };
}

function runFixtures(diagnostics) {
  const cases = buildFixtureCases();
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-request-flow-'));
  let failed = 0;
  try {
    for (const c of cases) {
      const dir = path.join(tmp, c.name.replace(/\W+/g, '-'));
      const requestDir = path.join(dir, '.missiond', 'requests', 'fixture');
      c.writeFiles({ requestDir });
      const observed = evaluateRequestDir(requestDir, {
        mode: c.mode,
        executeRequested: c.executeRequested,
      });
      const fxFails = [];
      if (observed.state !== c.expect.state) {
        fxFails.push(`expected state=${c.expect.state}, got ${observed.state}`);
      }
      if (observed.artifactKind !== c.expect.artifactKind) {
        fxFails.push(`expected artifact_kind=${c.expect.artifactKind}, got ${observed.artifactKind}`);
      }
      if (
        Array.isArray(c.expect.allowed)
        && JSON.stringify(observed.allowed) !== JSON.stringify(c.expect.allowed)
      ) {
        fxFails.push(
          `expected allowed=${JSON.stringify(c.expect.allowed)}, got ${JSON.stringify(observed.allowed)}`,
        );
      }
      if (Array.isArray(c.expect.refDiagnostics)) {
        if (
          JSON.stringify(observed.refDiagnostics) !== JSON.stringify(c.expect.refDiagnostics)
        ) {
          fxFails.push(
            `expected refDiagnostics=${JSON.stringify(c.expect.refDiagnostics)}, got ${JSON.stringify(observed.refDiagnostics)}`,
          );
        }
      } else if (typeof c.expect.refDiagnosticsContain === 'string') {
        const found = observed.refDiagnostics.some((d) => d.includes(c.expect.refDiagnosticsContain));
        if (!found) {
          fxFails.push(
            `expected refDiagnostics to include "${c.expect.refDiagnosticsContain}", got ${JSON.stringify(observed.refDiagnostics)}`,
          );
        }
      }
      if (fxFails.length > 0) {
        failed += 1;
        diagnostics.push({
          file: 'fixture',
          message: `fixture FAILED [${c.name}]: ${fxFails.join('; ')}`,
        });
      }
    }
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  return { totalCases: cases.length, failedCases: failed };
}

// ── main ───────────────────────────────────────────────────────────────

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const optsValidation = validateOpts(opts);
  if (!optsValidation.ok) {
    for (const e of optsValidation.errors) process.stderr.write(`error: ${e}\n`);
    process.stderr.write(`\n${usage}`);
    process.exit(2);
  }
  const diagnostics = [];

  let blueprintResult = { ok: true, diagnostics: [] };
  let handlerResult = { ok: true, diagnostics: [] };
  let mcpResult = { ok: true, diagnostics: [] };

  if (!opts.dryFixture) {
    const blueprintAbs = path.resolve(opts.repo, opts.blueprint);
    let blueprintSource;
    try {
      blueprintSource = fs.readFileSync(blueprintAbs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: opts.blueprint, message: `cannot read blueprint: ${err.message}` });
      blueprintSource = null;
    }
    if (blueprintSource != null) {
      let forms;
      try {
        forms = parseLisp(blueprintSource, blueprintAbs);
      } catch (err) {
        diagnostics.push({
          file: opts.blueprint,
          message: `blueprint parse error: ${err.message}`,
        });
        forms = null;
      }
      if (forms != null) {
        blueprintResult = validateBlueprintAst(forms, opts.blueprint);
        diagnostics.push(...blueprintResult.diagnostics);
      }
    }

    try {
      const sources = REQUEST_HANDLER_PATHS.map((rel) => {
        const abs = path.resolve(opts.repo, rel);
        return fs.readFileSync(abs, 'utf8');
      });
      const surfaceLabel = REQUEST_HANDLER_PATHS.join(' + ');
      handlerResult = validateRequestHandlerSource(sources.join('\n'), surfaceLabel);
      diagnostics.push(...handlerResult.diagnostics);
    } catch (err) {
      diagnostics.push({ file: REQUEST_HANDLER_PATHS.join(' + '), message: `cannot read: ${err.message}` });
      handlerResult = { ok: false };
    }

    const mcpAbs = path.resolve(opts.repo, MCP_REQUEST_PATH);
    try {
      const src = fs.readFileSync(mcpAbs, 'utf8');
      mcpResult = validateMcpRequestSource(src, MCP_REQUEST_PATH);
      diagnostics.push(...mcpResult.diagnostics);
    } catch (err) {
      diagnostics.push({ file: MCP_REQUEST_PATH, message: `cannot read: ${err.message}` });
      mcpResult = { ok: false };
    }
  }

  const fxSummary = runFixtures(diagnostics);
  const restartFxSummary = runRestartRecoveryFixtures(diagnostics);
  fxSummary.totalCases += restartFxSummary.totalCases;
  fxSummary.failedCases += restartFxSummary.failedCases;

  let liveIpcSummary = null;
  if (opts.liveIpc) {
    try {
      liveIpcSummary = await runLiveIpcSmoke(opts);
    } catch (err) {
      liveIpcSummary = {
        attempted: true,
        ok: false,
        error: err.message ?? String(err),
        steps: [],
      };
    }
    if (!liveIpcSummary.ok) {
      diagnostics.push({
        file: 'live-ipc',
        message: `live IPC smoke FAILED: ${liveIpcSummary.error ?? 'see steps below'}`,
      });
      for (const step of liveIpcSummary.steps ?? []) {
        if (!step.ok) {
          diagnostics.push({
            file: `live-ipc:${step.name}`,
            message: step.error ?? 'step failed',
          });
        }
      }
    }
  }

  const ok = diagnostics.length === 0;
  let modeLabel;
  if (opts.dryFixture) modeLabel = 'dry-fixture';
  else if (opts.liveIpc) modeLabel = 'static+fixture+live-ipc';
  else modeLabel = 'static+fixture';

  const result = {
    ok,
    mode: modeLabel,
    blueprint: opts.blueprint,
    expected_states: EXPECTED_STATES,
    expected_rules: EXPECTED_RULE_HEADS,
    expected_decisions: EXPECTED_DECISIONS,
    fixture_total: fxSummary.totalCases,
    fixture_failed: fxSummary.failedCases,
    diagnostics,
    live_ipc: liveIpcSummary,
  };

  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (ok) {
    const liveTag = liveIpcSummary
      ? `, live-ipc ${liveIpcSummary.steps?.length ?? 0} steps OK on request_id=${liveIpcSummary.request_id}`
      : '';
    console.log(
      `v3 request-flow smoke OK (${result.mode}, ${fxSummary.totalCases} fixtures, ${EXPECTED_STATES.length} states, ${EXPECTED_DECISIONS.length} decisions${liveTag})`,
    );
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(`v3 request-flow smoke FAILED — ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(ok ? 0 : 1);
}

// ── Live IPC smoke ─────────────────────────────────────────────────────
//
// Drives the real V3 mission_request flow against a running daemon:
//   1. action=start with compiler_mode=dry_run, persist=true, write_request_file=true,
//      review_gate_policy=manual. Asserts request.lisp + intent-alignment.lisp exist
//      and review_packet.state = "awaiting_intent_approval".
//   2. action=respond response=approve_intent. Asserts plan.lisp exists, contains
//      executable routing hints (:nodes/:target/:objective), and review_packet.state =
//      "awaiting_plan_approval".
//   3. action=respond response=approve_plan. Asserts plan.lisp now contains
//      :plan_id/:version/:board_task_id (post-approval write-back) and
//      review_packet.state = "awaiting_execution" with execute_allowed=true and
//      execute_plan in allowed_responses.
//
// Stops there. Never calls execute_plan; never dispatches a workstation slot.
async function runLiveIpcSmoke(opts) {
  const endpoint = opts.endpoint || defaultIpcEndpoint();
  const sessionId = opts.sessionId || defaultLiveSessionId();
  const requestId = opts.requestId || generateLiveRequestId();
  const repoRoot = path.resolve(opts.repo);
  const requestDir = path.join(repoRoot, '.missiond', 'requests', requestId);
  const requestPath = path.join(requestDir, 'request.lisp');
  const intentPath = path.join(requestDir, 'intent-alignment.lisp');
  const planPath = path.join(requestDir, 'plan.lisp');
  const alignmentRoot = path.join(repoRoot, '.missiond', 'alignment');
  const plansRoot = path.join(repoRoot, '.missiond', 'plans');
  const compatRequested = !!opts.compatWriteFile;
  // wave47-01: when --execute-real-dispatch is on, the smoke objective is
  // forwarded all the way through to the workstation_dispatch substrate's
  // task_brief (Objective section). It MUST tell the delegated worker not
  // to edit any files and not to commit, so the BoardTask Autopilot picks
  // up performs a read-only confirmation only. owned_files stays empty so
  // classify_task_kind→ReadOnly and the brief instructs commit_status=
  // not-required (see workstation_dispatch.rs::build_task_brief).
  const realDispatchObjective = (
    'WAVE47 REAL-DISPATCH SMOKE — READ-ONLY. '
    + 'DO NOT edit any files. DO NOT git add. DO NOT git commit. '
    + 'DO NOT modify the worktree or any external service. '
    + 'Run `git status --short` and `git rev-parse HEAD`, capture the output verbatim, '
    + 'then call mission_execution(action=complete) with enforce_scoped_commit=true, '
    + 'commit_status=not-required, and a one-line summary explaining no commit was produced. '
    + 'This is a real-dispatch substrate audit; do not attempt to satisfy any acceptance command.'
  );
  const smokeObjective = opts.executeRealDispatch
    ? realDispatchObjective
    : compatRequested
      ? 'wave44-live-ipc-smoke compat-write-file path; opt-in legacy compat artifacts.'
      : 'wave44-live-ipc-smoke request-local-only path; no compat artifacts expected.';

  const compatBefore = {
    alignment_subdirs: snapshotSubdirs(alignmentRoot),
    plans_subdirs: snapshotSubdirs(plansRoot),
  };

  const summary = {
    attempted: true,
    ok: false,
    endpoint,
    session_id: sessionId,
    request_id: requestId,
    project_root: repoRoot,
    request_dir: requestDir,
    compat_write_file_requested: compatRequested,
    execute_dry_run_requested: !!opts.executeDryRun,
    execute_real_dispatch_requested: !!opts.executeRealDispatch,
    restart_during_dispatch_requested: !!opts.restartDuringDispatch,
    confirm_execute_refused: !!opts.confirmExecute,
    confirm_execute_notice: opts.confirmExecute
      ? '--confirm-execute is reserved; this checker still refuses to dispatch a workstation slot. Drive mission_request directly to execute.'
      : null,
    steps: [],
    cleanup: { requested: !!opts.cleanup, removed_path: null, kept_db_rows: 'directives, plans, board_tasks rows created during the live flow remain as audit records — only the request-local Lisp directory is cleaned up.' },
    legacy_compat_artifacts: null,
    restart_recovery_plan: null,
    error: null,
  };

  const callTool = (name, args) => callToolViaIpc({
    endpoint,
    sessionId,
    name,
    arguments: args,
    timeoutMs: 60_000,
  });

  // ── Step 1: action=start ─────────────────────────────────────────────
  // Default flow does NOT pass write_file=true: per V3
  // (compat-writer-policy ...), request-local artifacts are the SSOT and
  // .missiond/alignment/ + .missiond/plans/ compat writes are opt-in via
  // --compat-write-file (which sets compat_write_file=true on the call).
  const startArgs = {
    action: 'start',
    request_id: requestId,
    message: smokeObjective,
    mode: 'human_interactive',
    cwd: repoRoot,
    compiler_mode: 'dry_run',
    persist: true,
    write_request_file: true,
    overwrite_file: true,
    review_gate_policy: 'manual',
    target: 'mission_task_delegate',
    objective: smokeObjective,
  };
  if (compatRequested) {
    startArgs.compat_write_file = true;
  }

  let startStep = { name: 'start', ok: false };
  try {
    const startRaw = await callTool('mission_request', startArgs);
    const parsed = parseToolResultPayload(startRaw, 'start');
    if (!parsed.ok) {
      startStep.error = parsed.error;
      summary.steps.push(startStep);
      summary.error = parsed.error;
      return summary;
    }
    if (parsed.isError) {
      startStep.error = `start returned is_error=true: ${JSON.stringify(parsed.payload)}`;
      summary.steps.push(startStep);
      summary.error = startStep.error;
      return summary;
    }
    const requestExists = fs.existsSync(requestPath);
    const intentExists = fs.existsSync(intentPath);
    const reviewPacket = parsed.payload?.review_packet ?? null;
    startStep = {
      name: 'start',
      ok: false,
      request_path: requestPath,
      intent_path: intentPath,
      request_exists: requestExists,
      intent_exists: intentExists,
      review_packet_state: reviewPacket?.state ?? null,
      review_packet_artifact_kind: reviewPacket?.artifact_kind ?? null,
      review_packet_execute_allowed: reviewPacket?.execute_allowed ?? null,
      payload_status: parsed.payload?.status ?? null,
    };
    const fails = [];
    if (!requestExists) fails.push(`request.lisp absent at ${requestPath}`);
    if (!intentExists) fails.push(`intent-alignment.lisp absent at ${intentPath}`);
    if (reviewPacket?.state !== 'awaiting_intent_approval') {
      fails.push(`expected review_packet.state=awaiting_intent_approval, got ${JSON.stringify(reviewPacket?.state)}`);
    }
    if (fails.length === 0) {
      startStep.ok = true;
    } else {
      startStep.error = fails.join('; ');
    }
    summary.steps.push(startStep);
    if (!startStep.ok) {
      summary.error = startStep.error;
      return summary;
    }
  } catch (err) {
    startStep.error = err.message ?? String(err);
    summary.steps.push(startStep);
    summary.error = startStep.error;
    return summary;
  }

  // ── Step 2: action=respond response=approve_intent ──────────────────
  let approveIntentStep = { name: 'approve_intent', ok: false };
  try {
    const approveIntentArgs = {
      action: 'respond',
      request_id: requestId,
      response: 'approve_intent',
      cwd: repoRoot,
      compiler_mode: 'dry_run',
      persist: true,
      overwrite_file: true,
      review_gate_policy: 'manual',
      target: 'mission_task_delegate',
      objective: smokeObjective,
    };
    if (compatRequested) {
      approveIntentArgs.compat_write_file = true;
    }
    const respondRaw = await callTool('mission_request', approveIntentArgs);
    const parsed = parseToolResultPayload(respondRaw, 'approve_intent');
    if (!parsed.ok) {
      approveIntentStep.error = parsed.error;
      summary.steps.push(approveIntentStep);
      summary.error = parsed.error;
      return summary;
    }
    if (parsed.isError) {
      approveIntentStep.error = `approve_intent returned is_error=true: ${JSON.stringify(parsed.payload?.respond_result ?? parsed.payload).slice(0, 600)}`;
      summary.steps.push(approveIntentStep);
      summary.error = approveIntentStep.error;
      return summary;
    }
    const planExists = fs.existsSync(planPath);
    let planText = '';
    if (planExists) planText = fs.readFileSync(planPath, 'utf8');
    const reviewPacket = parsed.payload?.review_packet ?? null;
    const respondResult = parsed.payload?.respond_result ?? null;
    const planHasNodes = planText.includes(':nodes');
    const planHasTarget = planText.includes(':target');
    const planHasObjective = planText.includes(':objective');
    approveIntentStep = {
      name: 'approve_intent',
      ok: false,
      respond_outcome: respondResult?.outcome ?? null,
      respond_inner_action: respondResult?.inner_action ?? null,
      plan_path: planPath,
      plan_exists: planExists,
      plan_has_target: planHasTarget,
      plan_has_objective: planHasObjective,
      plan_has_nodes: planHasNodes,
      review_packet_state: reviewPacket?.state ?? null,
      review_packet_artifact_kind: reviewPacket?.artifact_kind ?? null,
      review_packet_execute_allowed: reviewPacket?.execute_allowed ?? null,
    };
    const fails = [];
    if (respondResult?.outcome !== 'dispatched') {
      fails.push(`expected approve_intent outcome=dispatched, got ${JSON.stringify(respondResult?.outcome)}; blocked_reason=${JSON.stringify(respondResult?.blocked_reason)}`);
    }
    if (!planExists) fails.push(`plan.lisp absent at ${planPath} after approve_intent`);
    if (planExists && !(planHasNodes || planHasTarget || planHasObjective)) {
      fails.push('plan.lisp lacks executable routing hints (:nodes / :target / :objective)');
    }
    if (reviewPacket?.state !== 'awaiting_plan_approval') {
      fails.push(`expected review_packet.state=awaiting_plan_approval, got ${JSON.stringify(reviewPacket?.state)}`);
    }
    if (fails.length === 0) {
      approveIntentStep.ok = true;
    } else {
      approveIntentStep.error = fails.join('; ');
    }
    summary.steps.push(approveIntentStep);
    if (!approveIntentStep.ok) {
      summary.error = approveIntentStep.error;
      return summary;
    }
  } catch (err) {
    approveIntentStep.error = err.message ?? String(err);
    summary.steps.push(approveIntentStep);
    summary.error = approveIntentStep.error;
    return summary;
  }

  // ── Step 3: action=respond response=approve_plan ────────────────────
  let approvePlanStep = { name: 'approve_plan', ok: false };
  try {
    const respondRaw = await callTool('mission_request', {
      action: 'respond',
      request_id: requestId,
      response: 'approve_plan',
      cwd: repoRoot,
    });
    const parsed = parseToolResultPayload(respondRaw, 'approve_plan');
    if (!parsed.ok) {
      approvePlanStep.error = parsed.error;
      summary.steps.push(approvePlanStep);
      summary.error = parsed.error;
      return summary;
    }
    if (parsed.isError) {
      approvePlanStep.error = `approve_plan returned is_error=true: ${JSON.stringify(parsed.payload?.respond_result ?? parsed.payload).slice(0, 600)}`;
      summary.steps.push(approvePlanStep);
      summary.error = approvePlanStep.error;
      return summary;
    }
    const planText = fs.existsSync(planPath) ? fs.readFileSync(planPath, 'utf8') : '';
    const planId = extractLispKeywordString(planText, 'plan_id');
    const boardTaskId = extractLispKeywordString(planText, 'board_task_id');
    const versionMatch = planText.match(/:version\s+(\d+)/);
    const reviewPacket = parsed.payload?.review_packet ?? null;
    const respondResult = parsed.payload?.respond_result ?? null;
    const allowed = Array.isArray(reviewPacket?.allowed_responses)
      ? reviewPacket.allowed_responses
      : [];
    approvePlanStep = {
      name: 'approve_plan',
      ok: false,
      respond_outcome: respondResult?.outcome ?? null,
      respond_inner_action: respondResult?.inner_action ?? null,
      plan_materialized: respondResult?.plan_materialized ?? false,
      plan_id: planId,
      plan_version: versionMatch ? Number.parseInt(versionMatch[1], 10) : null,
      plan_board_task_id: boardTaskId,
      review_packet_state: reviewPacket?.state ?? null,
      review_packet_execute_allowed: reviewPacket?.execute_allowed ?? null,
      allowed_responses: allowed,
    };
    const fails = [];
    if (respondResult?.outcome !== 'dispatched') {
      fails.push(`expected approve_plan outcome=dispatched, got ${JSON.stringify(respondResult?.outcome)}; blocked_reason=${JSON.stringify(respondResult?.blocked_reason)}`);
    }
    if (!planId) fails.push('plan.lisp missing :plan_id stamp after approve_plan');
    if (!boardTaskId) fails.push('plan.lisp missing :board_task_id stamp after approve_plan');
    if (!versionMatch) fails.push('plan.lisp missing :version stamp after approve_plan');
    if (reviewPacket?.state !== 'awaiting_execution') {
      fails.push(`expected review_packet.state=awaiting_execution, got ${JSON.stringify(reviewPacket?.state)}`);
    }
    if (reviewPacket?.execute_allowed !== true) {
      fails.push(`expected review_packet.execute_allowed=true, got ${JSON.stringify(reviewPacket?.execute_allowed)}`);
    }
    if (!allowed.includes('execute_plan')) {
      fails.push(`expected allowed_responses to include execute_plan, got ${JSON.stringify(allowed)}`);
    }
    if (fails.length === 0) {
      approvePlanStep.ok = true;
    } else {
      approvePlanStep.error = fails.join('; ');
    }
    summary.steps.push(approvePlanStep);
    if (!approvePlanStep.ok) {
      summary.error = approvePlanStep.error;
      return summary;
    }
  } catch (err) {
    approvePlanStep.error = err.message ?? String(err);
    summary.steps.push(approvePlanStep);
    summary.error = approvePlanStep.error;
    return summary;
  }

  // ── Step 4 (opt-in): action=respond response=execute_plan dry_run=true ──
  // Wave46-01 audit mode. Default --live-ipc keeps stopping at
  // awaiting_execution; only --execute-dry-run drives execute_plan. When
  // opted in, the smoke MUST pass execute_mode=internal +
  // dispatch_strategy=agent-team + dry_run=true + target=mission_task_delegate
  // so mission_plan's `action_execute_internal` path reaches
  // `run_workstation_dispatch_with_contract_and_trace` and returns the
  // `WorkstationDispatchOutcome::DryRun` shape (status=dry_run,
  // runner_status=workstation_dispatch_v0,
  // workstation_dispatch_status=dry_run_no_dispatch, task_brief_preview
  // present). Bridge mode is no longer accepted as a no-dispatch proof
  // because it short-circuits before the substrate runs. The smoke never
  // spawns or waits for a worker.
  if (opts.executeDryRun) {
    let executeStep = { name: 'execute_plan_dry_run', ok: false };
    const eventsDir = path.join(requestDir, 'events');
    const eventsBefore = fs.existsSync(eventsDir)
      ? fs.readdirSync(eventsDir).filter((n) => n.endsWith('.event.lisp')).sort()
      : [];
    try {
      const respondRaw = await callTool('mission_request', {
        action: 'respond',
        request_id: requestId,
        response: 'execute_plan',
        execute: true,
        dry_run: true,
        execute_mode: 'internal',
        dispatch_strategy: 'agent-team',
        cwd: repoRoot,
        target: 'mission_task_delegate',
        objective: smokeObjective,
      });
      const parsed = parseToolResultPayload(respondRaw, 'execute_plan');
      if (!parsed.ok) {
        executeStep.error = parsed.error;
        summary.steps.push(executeStep);
        summary.error = parsed.error;
        return summary;
      }
      // Note: execute_plan can come back with is_error=true if the inner
      // execute returned an error structure; we surface the full payload so
      // the assertion logic below can decide.
      const respondResult = parsed.payload?.respond_result ?? null;
      const reviewPacket = parsed.payload?.review_packet ?? null;
      const pipelineResult = parsed.payload?.pipeline_result ?? null;
      // pipeline_result is the already-hydrated inner JSON
      // (tool_result_payload in request.rs runs serde_json::from_str on the
      // inner ToolResult text). Read substrate fields directly.
      const status = pipelineResult?.status ?? null;
      const runnerStatus = pipelineResult?.runner_status ?? null;
      const executeMode = pipelineResult?.execute_mode ?? null;
      const workstationDispatchStatus =
        pipelineResult?.workstation_dispatch_status ?? null;
      const targetTool = pipelineResult?.target_tool ?? null;
      const pipelineDispatchStrategy =
        pipelineResult?.dispatch_strategy ?? null;
      const taskBriefPreview = pipelineResult?.task_brief_preview ?? null;
      const taskBriefPreviewPresent =
        typeof taskBriefPreview === 'string' && taskBriefPreview.length > 0;
      const allowed = Array.isArray(reviewPacket?.allowed_responses)
        ? reviewPacket.allowed_responses
        : [];
      const eventsAfter = fs.existsSync(eventsDir)
        ? fs.readdirSync(eventsDir).filter((n) => n.endsWith('.event.lisp')).sort()
        : [];
      const newEvents = eventsAfter.filter((n) => !eventsBefore.includes(n));
      const newEventTexts = newEvents.map((n) =>
        fs.readFileSync(path.join(eventsDir, n), 'utf8'),
      );
      const executeEventAppended = newEventTexts.some((t) =>
        t.includes(':decision :execute_plan'),
      );
      // wave46: no-dispatch proof now requires the workstation_dispatch
      // substrate's dry-run shape. Bridge mode is no longer accepted.
      const noDispatchProof = (
        status === 'dry_run'
        && runnerStatus === 'workstation_dispatch_v0'
        && workstationDispatchStatus === 'dry_run_no_dispatch'
      );

      executeStep = {
        name: 'execute_plan_dry_run',
        ok: false,
        respond_outcome: respondResult?.outcome ?? null,
        respond_inner_action: respondResult?.inner_action ?? null,
        respond_result_execute: respondResult?.execute ?? null,
        review_packet_state: reviewPacket?.state ?? null,
        review_packet_execute_allowed: reviewPacket?.execute_allowed ?? null,
        allowed_responses: allowed,
        new_events: newEvents,
        execute_event_appended: executeEventAppended,
        pipeline_status: status,
        pipeline_runner_status: runnerStatus,
        pipeline_execute_mode: executeMode,
        pipeline_workstation_dispatch_status: workstationDispatchStatus,
        pipeline_target_tool: targetTool,
        pipeline_dispatch_strategy: pipelineDispatchStrategy,
        pipeline_task_brief_preview_present: taskBriefPreviewPresent,
        no_dispatch_proof: noDispatchProof,
      };
      const fails = [];
      if (respondResult?.outcome !== 'dispatched') {
        fails.push(`expected execute_plan outcome=dispatched, got ${JSON.stringify(respondResult?.outcome)}; blocked_reason=${JSON.stringify(respondResult?.blocked_reason)}`);
      }
      if (respondResult?.inner_action !== 'unified_entry::plan_execute') {
        fails.push(`expected inner_action=unified_entry::plan_execute, got ${JSON.stringify(respondResult?.inner_action)}`);
      }
      if (respondResult?.execute !== true) {
        fails.push(`expected respond_result.execute=true, got ${JSON.stringify(respondResult?.execute)}`);
      }
      if (reviewPacket?.state !== 'execute_requested') {
        fails.push(`expected review_packet.state=execute_requested, got ${JSON.stringify(reviewPacket?.state)}`);
      }
      if (allowed.length !== 1 || allowed[0] !== 'observe') {
        fails.push(`expected allowed_responses=[observe], got ${JSON.stringify(allowed)}`);
      }
      if (!executeEventAppended) {
        fails.push(`expected a request-local execute_plan review event under ${eventsDir}; new events=${JSON.stringify(newEvents)}`);
      }
      if (executeMode !== 'internal') {
        fails.push(`expected pipeline_result.execute_mode='internal', got ${JSON.stringify(executeMode)}`);
      }
      if (status !== 'dry_run') {
        fails.push(`expected pipeline_result.status='dry_run', got ${JSON.stringify(status)}`);
      }
      if (runnerStatus !== 'workstation_dispatch_v0') {
        fails.push(`expected pipeline_result.runner_status='workstation_dispatch_v0', got ${JSON.stringify(runnerStatus)}`);
      }
      if (workstationDispatchStatus !== 'dry_run_no_dispatch') {
        fails.push(`expected pipeline_result.workstation_dispatch_status='dry_run_no_dispatch', got ${JSON.stringify(workstationDispatchStatus)}`);
      }
      if (targetTool !== 'mission_task_delegate') {
        fails.push(`expected pipeline_result.target_tool='mission_task_delegate', got ${JSON.stringify(targetTool)}`);
      }
      if (pipelineDispatchStrategy !== 'agent-team') {
        fails.push(`expected pipeline_result.dispatch_strategy='agent-team', got ${JSON.stringify(pipelineDispatchStrategy)}`);
      }
      if (!taskBriefPreviewPresent) {
        fails.push(`expected pipeline_result.task_brief_preview to be a non-empty string; got ${JSON.stringify(taskBriefPreview)}`);
      }
      if (!noDispatchProof) {
        fails.push(`expected workstation-dispatch substrate no-dispatch proof (status=dry_run + runner_status=workstation_dispatch_v0 + workstation_dispatch_status=dry_run_no_dispatch); got status=${JSON.stringify(status)}, runner_status=${JSON.stringify(runnerStatus)}, workstation_dispatch_status=${JSON.stringify(workstationDispatchStatus)}`);
      }
      if (fails.length === 0) {
        executeStep.ok = true;
      } else {
        executeStep.error = fails.join('; ');
      }
      summary.steps.push(executeStep);
      if (!executeStep.ok) {
        summary.error = executeStep.error;
        return summary;
      }
    } catch (err) {
      executeStep.error = err.message ?? String(err);
      summary.steps.push(executeStep);
      summary.error = executeStep.error;
      return summary;
    }
  }

  // ── Step 4b (opt-in): action=respond response=execute_plan dry_run=false ──
  // Wave47-01 audit mode (--execute-real-dispatch). SLOW + SIDE-EFFECTING.
  // Drives the workstation_dispatch substrate end-to-end with dry_run=false
  // so `run_workstation_dispatch_with_contract_and_trace` calls
  // mission_task_delegate, which creates a delegated BoardTask via
  // state.store.create_board_task and notifies the dispatcher. The smoke
  // pins the wire shape (status='executing', runner_status='workstation_
  // dispatch_v0', workstation_dispatch_status='dispatched',
  // delegated_board_task_id present as a UUID, task_brief_preview present)
  // WITHOUT waiting on the worker. The created BoardTask stays in the
  // queue for Autopilot to pick up. NEVER part of the aggregate v3 gate.
  // This block is SKIPPED whenever --execute-real-dispatch is absent, so
  // default and --execute-dry-run runs cannot real-dispatch.
  if (opts.executeRealDispatch) {
    let realStep = { name: 'execute_plan_real_dispatch', ok: false };
    const eventsDir = path.join(requestDir, 'events');
    const eventsBefore = fs.existsSync(eventsDir)
      ? fs.readdirSync(eventsDir).filter((n) => n.endsWith('.event.lisp')).sort()
      : [];
    try {
      const respondRaw = await callTool('mission_request', {
        action: 'respond',
        request_id: requestId,
        response: 'execute_plan',
        execute: true,
        dry_run: false,
        execute_mode: 'internal',
        dispatch_strategy: 'agent-team',
        cwd: repoRoot,
        target: 'mission_task_delegate',
        objective: smokeObjective,
      });
      const parsed = parseToolResultPayload(respondRaw, 'execute_plan');
      if (!parsed.ok) {
        realStep.error = parsed.error;
        summary.steps.push(realStep);
        summary.error = parsed.error;
        return summary;
      }
      const respondResult = parsed.payload?.respond_result ?? null;
      const reviewPacket = parsed.payload?.review_packet ?? null;
      const pipelineResult = parsed.payload?.pipeline_result ?? null;
      const status = pipelineResult?.status ?? null;
      const runnerStatus = pipelineResult?.runner_status ?? null;
      const executeMode = pipelineResult?.execute_mode ?? null;
      const workstationDispatchStatus =
        pipelineResult?.workstation_dispatch_status ?? null;
      const targetTool = pipelineResult?.target_tool ?? null;
      const pipelineDispatchStrategy =
        pipelineResult?.dispatch_strategy ?? null;
      const taskBriefPreview = pipelineResult?.task_brief_preview ?? null;
      const taskBriefPreviewPresent =
        typeof taskBriefPreview === 'string' && taskBriefPreview.length > 0;
      const innerResult = pipelineResult?.inner_result ?? null;
      const innerResultPresent =
        innerResult !== null
        && typeof innerResult === 'object'
        && !Array.isArray(innerResult);
      // wave47-01: workstation_dispatch.rs::extract_inner_board_task_id
      // projects a stable top-level `delegated_board_task_id` UUID string
      // onto pipeline_result for the Dispatched outcome. The inner
      // mission_task_delegate response also carries the BoardTask under
      // `inner_result.task_id` (currently the FULL DB row because
      // compute/task_delegate.rs::handle shadows the variable name —
      // `task_id = state.store.create_board_task(...)` returns the row).
      // Prefer the projected field; fall back to inner_result.task_id.id
      // (current daemon behaviour) and inner_result.task_id (defensive
      // fallback if compute/task_delegate.rs is later tightened to
      // surface a string).
      const projectedDelegatedId =
        pipelineResult?.delegated_board_task_id ?? null;
      const innerTaskIdRaw = innerResultPresent
        ? (innerResult.task_id ?? null)
        : null;
      const innerTaskIdNested =
        innerTaskIdRaw && typeof innerTaskIdRaw === 'object'
          ? (innerTaskIdRaw.id ?? null)
          : null;
      const innerTaskIdString =
        typeof innerTaskIdRaw === 'string' ? innerTaskIdRaw : null;
      const delegatedBoardTaskId =
        (typeof projectedDelegatedId === 'string'
          ? projectedDelegatedId
          : null)
        ?? innerTaskIdNested
        ?? innerTaskIdString;
      const uuidRegex =
        /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
      const delegatedBoardTaskIdLooksLikeUuid =
        typeof delegatedBoardTaskId === 'string'
        && uuidRegex.test(delegatedBoardTaskId);
      // BoardTask row status is camelCase per the store's serde derives;
      // accept either snake or camel for the queued/open status field.
      const innerBoardTaskStatus =
        (innerTaskIdRaw && typeof innerTaskIdRaw === 'object'
          ? (innerTaskIdRaw.status ?? null)
          : null)
        ?? (innerResultPresent ? (innerResult.status ?? null) : null);
      const innerAssignee = innerResultPresent
        ? (innerResult.assignee ?? null)
        : null;
      const innerProvisionedNewSlot = innerResultPresent
        ? (innerResult.provisioned_new_slot ?? null)
        : null;
      const allowed = Array.isArray(reviewPacket?.allowed_responses)
        ? reviewPacket.allowed_responses
        : [];
      const eventsAfter = fs.existsSync(eventsDir)
        ? fs.readdirSync(eventsDir).filter((n) => n.endsWith('.event.lisp')).sort()
        : [];
      const newEvents = eventsAfter.filter((n) => !eventsBefore.includes(n));
      const newEventTexts = newEvents.map((n) =>
        fs.readFileSync(path.join(eventsDir, n), 'utf8'),
      );
      const executeEventAppended = newEventTexts.some((t) =>
        t.includes(':decision :execute_plan'),
      );
      // wave-15 substrate Dispatched semantics (see plan.rs::
      // build_workstation_dispatch_response): the response status reflects
      // the plan FSM transition triggered by a successful dispatch
      // ("executing"), while the substrate-level invariant is carried by
      // workstation_dispatch_status="dispatched". Both are required for a
      // real-dispatch proof.
      const dispatchProof = (
        status === 'executing'
        && runnerStatus === 'workstation_dispatch_v0'
        && workstationDispatchStatus === 'dispatched'
      );

      realStep = {
        name: 'execute_plan_real_dispatch',
        ok: false,
        respond_outcome: respondResult?.outcome ?? null,
        respond_inner_action: respondResult?.inner_action ?? null,
        respond_result_execute: respondResult?.execute ?? null,
        review_packet_state: reviewPacket?.state ?? null,
        review_packet_execute_allowed: reviewPacket?.execute_allowed ?? null,
        allowed_responses: allowed,
        new_events: newEvents,
        execute_event_appended: executeEventAppended,
        pipeline_status: status,
        pipeline_runner_status: runnerStatus,
        pipeline_execute_mode: executeMode,
        pipeline_workstation_dispatch_status: workstationDispatchStatus,
        pipeline_target_tool: targetTool,
        pipeline_dispatch_strategy: pipelineDispatchStrategy,
        pipeline_task_brief_preview_present: taskBriefPreviewPresent,
        pipeline_inner_result_present: innerResultPresent,
        pipeline_delegated_board_task_id_projected:
          typeof projectedDelegatedId === 'string'
            ? projectedDelegatedId
            : null,
        delegated_board_task_id: delegatedBoardTaskId,
        delegated_board_task_status: innerBoardTaskStatus,
        delegated_board_task_assignee: innerAssignee,
        delegated_board_task_provisioned_new_slot: innerProvisionedNewSlot,
        dispatch_proof: dispatchProof,
        autopilot_handoff_note:
          'BoardTask is queued; Autopilot drives it. The smoke does not wait. '
          + 'Use mission_board_get(taskId=<delegated_board_task_id>) to observe progress.',
      };
      const fails = [];
      if (respondResult?.outcome !== 'dispatched') {
        fails.push(`expected execute_plan outcome=dispatched, got ${JSON.stringify(respondResult?.outcome)}; blocked_reason=${JSON.stringify(respondResult?.blocked_reason)}`);
      }
      if (respondResult?.inner_action !== 'unified_entry::plan_execute') {
        fails.push(`expected inner_action=unified_entry::plan_execute, got ${JSON.stringify(respondResult?.inner_action)}`);
      }
      if (respondResult?.execute !== true) {
        fails.push(`expected respond_result.execute=true, got ${JSON.stringify(respondResult?.execute)}`);
      }
      if (reviewPacket?.state !== 'execute_requested') {
        fails.push(`expected review_packet.state=execute_requested, got ${JSON.stringify(reviewPacket?.state)}`);
      }
      if (allowed.length !== 1 || allowed[0] !== 'observe') {
        fails.push(`expected allowed_responses=[observe], got ${JSON.stringify(allowed)}`);
      }
      if (!executeEventAppended) {
        fails.push(`expected a request-local execute_plan review event under ${eventsDir}; new events=${JSON.stringify(newEvents)}`);
      }
      if (executeMode !== 'internal') {
        fails.push(`expected pipeline_result.execute_mode='internal', got ${JSON.stringify(executeMode)}`);
      }
      if (status !== 'executing') {
        fails.push(`expected pipeline_result.status='executing' (the plan FSM transitions to Executing on a successful workstation_dispatch substrate Dispatched outcome — see crates/missiond-daemon/src/handlers/knowledge/plan.rs::build_workstation_dispatch_response); got ${JSON.stringify(status)}`);
      }
      if (runnerStatus !== 'workstation_dispatch_v0') {
        fails.push(`expected pipeline_result.runner_status='workstation_dispatch_v0', got ${JSON.stringify(runnerStatus)}`);
      }
      if (workstationDispatchStatus !== 'dispatched') {
        fails.push(`expected pipeline_result.workstation_dispatch_status='dispatched', got ${JSON.stringify(workstationDispatchStatus)}`);
      }
      if (targetTool !== 'mission_task_delegate') {
        fails.push(`expected pipeline_result.target_tool='mission_task_delegate', got ${JSON.stringify(targetTool)}`);
      }
      if (pipelineDispatchStrategy !== 'agent-team') {
        fails.push(`expected pipeline_result.dispatch_strategy='agent-team', got ${JSON.stringify(pipelineDispatchStrategy)}`);
      }
      if (!taskBriefPreviewPresent) {
        fails.push(`expected pipeline_result.task_brief_preview to be a non-empty string; got ${JSON.stringify(taskBriefPreview)}`);
      }
      if (!innerResultPresent) {
        fails.push(`expected pipeline_result.inner_result to be a non-null object; got ${JSON.stringify(innerResult)}`);
      }
      if (!delegatedBoardTaskIdLooksLikeUuid) {
        fails.push(`expected a delegated BoardTask UUID at pipeline_result.delegated_board_task_id (preferred, projected by workstation_dispatch.rs::extract_inner_board_task_id) or pipeline_result.inner_result.task_id.id; got ${JSON.stringify(delegatedBoardTaskId)}. Real-dispatch substrate must surface a stable BoardTask id so observers can close the delegated task.`);
      }
      if (!dispatchProof) {
        fails.push(`expected workstation-dispatch substrate dispatch proof (status='executing' + runner_status='workstation_dispatch_v0' + workstation_dispatch_status='dispatched'); got status=${JSON.stringify(status)}, runner_status=${JSON.stringify(runnerStatus)}, workstation_dispatch_status=${JSON.stringify(workstationDispatchStatus)}`);
      }
      if (fails.length === 0) {
        realStep.ok = true;
      } else {
        realStep.error = fails.join('; ');
      }
      summary.steps.push(realStep);
      if (!realStep.ok) {
        summary.error = realStep.error;
        return summary;
      }
    } catch (err) {
      realStep.error = err.message ?? String(err);
      summary.steps.push(realStep);
      summary.error = realStep.error;
      return summary;
    }
  }

  // ── Step 4c (opt-in): wave49-01 restart-recovery PLAN emission ──────
  // Plan-only by default. The step captures the delegated BoardTask id and
  // pre-restart assignee from the realStep summary, builds a structured
  // restart-recovery plan, and pushes it onto summary.steps as a review
  // artifact. The actual daemon restart is parent-driven (see
  // plan.parent_run_command) so the operator can inspect the plan before
  // touching the supervisor. Validation already ensured we have both
  // --live-ipc and --execute-real-dispatch by this point.
  if (opts.restartDuringDispatch) {
    const realStepSnapshot = summary.steps.find(
      (s) => s.name === 'execute_plan_real_dispatch',
    );
    const planStep = {
      name: 'restart_recovery_plan',
      ok: false,
      kind: 'review',
      live_executed: false,
    };
    if (!realStepSnapshot || !realStepSnapshot.ok) {
      planStep.error =
        'restart-recovery plan requires a successful execute_plan_real_dispatch step; none observed.';
      summary.steps.push(planStep);
      summary.error = planStep.error;
      return summary;
    }
    const plan = buildRestartRecoveryPlan({
      delegatedBoardTaskId: realStepSnapshot.delegated_board_task_id ?? null,
      preRestartAssignee:
        realStepSnapshot.delegated_board_task_assignee ?? null,
      requestId,
      repoRoot,
    });
    summary.restart_recovery_plan = plan;
    planStep.ok = true;
    planStep.plan_step_count = plan.steps.length;
    planStep.parent_run_command_preferred = plan.parent_run_command.preferred;
    planStep.assertion =
      'restart-recovery plan emitted for review; live execution intentionally deferred to the parent/orchestrator (see restart_recovery_plan.parent_run_command).';
    summary.steps.push(planStep);
  }

  // ── Compat-writer side-effect audit ─────────────────────────────────
  // Snapshot .missiond/alignment/ and .missiond/plans/ after the live flow
  // and diff against the pre-run snapshot. The default smoke FAILS if any
  // new compat artifact appears that names this request_id or the smoke
  // objective; --compat-write-file flips that into a separate "you asked
  // for them" report (the user explicitly opted into legacy compat writes).
  const compatAfter = {
    alignment_subdirs: snapshotSubdirs(alignmentRoot),
    plans_subdirs: snapshotSubdirs(plansRoot),
  };
  const newAlignmentSubdirs = compatAfter.alignment_subdirs.filter(
    (n) => !compatBefore.alignment_subdirs.includes(n),
  );
  const newPlansSubdirs = compatAfter.plans_subdirs.filter(
    (n) => !compatBefore.plans_subdirs.includes(n),
  );
  // Plan compat dirs are named by plan UUID, not request_id; identify the
  // ones belonging to this request by searching their PLAN.lisp for the
  // smoke objective string.
  const newPlanCompatPaths = newPlansSubdirs
    .map((n) => path.join(plansRoot, n, 'PLAN.lisp'))
    .filter((p) => {
      try {
        return fs.existsSync(p) && fs.readFileSync(p, 'utf8').includes(smokeObjective);
      } catch {
        return false;
      }
    });
  const alignmentForThisRequest = newAlignmentSubdirs.includes(requestId)
    ? path.join(alignmentRoot, requestId, 'intent-alignment.lisp')
    : null;

  summary.legacy_compat_artifacts = {
    snapshot_root_alignment: alignmentRoot,
    snapshot_root_plans: plansRoot,
    new_alignment_subdirs: newAlignmentSubdirs,
    new_plan_subdirs: newPlansSubdirs,
    new_alignment_path_for_this_request: alignmentForThisRequest,
    new_plan_compat_paths_for_this_smoke: newPlanCompatPaths,
    compat_write_file_requested: compatRequested,
  };

  if (!compatRequested) {
    const compatFails = [];
    if (alignmentForThisRequest) {
      compatFails.push(
        `default flow leaked compat artifact ${alignmentForThisRequest}; .missiond/alignment/<request_id>/ must be opt-in via --compat-write-file`,
      );
    }
    if (newPlanCompatPaths.length > 0) {
      compatFails.push(
        `default flow leaked compat plan artifacts ${JSON.stringify(newPlanCompatPaths)}; .missiond/plans/<plan_id>/PLAN.lisp must be opt-in via --compat-write-file`,
      );
    }
    if (compatFails.length > 0) {
      summary.steps.push({
        name: 'compat_write_audit',
        ok: false,
        error: compatFails.join('; '),
      });
      summary.error = compatFails.join('; ');
      return summary;
    }
    summary.steps.push({
      name: 'compat_write_audit',
      ok: true,
      assertion: 'no .missiond/alignment/<request_id>/ and no .missiond/plans/*/PLAN.lisp containing the smoke objective were created',
    });
  } else {
    summary.steps.push({
      name: 'compat_write_audit',
      ok: true,
      assertion: '--compat-write-file requested; legacy compat artifacts are reported separately and are not a failure',
      compat_artifacts: {
        alignment_path: alignmentForThisRequest,
        plan_compat_paths: newPlanCompatPaths,
      },
    });
  }

  summary.ok = true;

  // ── Optional cleanup: remove only the request-local directory ───────
  if (opts.cleanup) {
    try {
      // Defensive: only remove inside <repo>/.missiond/requests/.
      const expectedPrefix = path.join(repoRoot, '.missiond', 'requests') + path.sep;
      if (!requestDir.startsWith(expectedPrefix)) {
        summary.cleanup.error = `refused to remove ${requestDir}; outside expected prefix`;
      } else if (fs.existsSync(requestDir)) {
        fs.rmSync(requestDir, { recursive: true, force: true });
        summary.cleanup.removed_path = requestDir;
      }
    } catch (err) {
      summary.cleanup.error = err.message ?? String(err);
    }
  }

  return summary;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((err) => {
    console.error(err.stack || err.message || String(err));
    process.exit(1);
  });
}
