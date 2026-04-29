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
//   3. The request handler (crates/missiond-daemon/.../request.rs) declares
//      every wire state and decision string the blueprint promises.
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
// CLI: node scripts/check-v3-request-flow-smoke.mjs [--json] [--dry-fixture]
//        [--blueprint <path>] [--repo <path>]
//        [--live-ipc [--endpoint <socket>] [--session-id <id>]
//                    [--request-id <id>] [--cleanup] [--compat-write-file]
//                    [--confirm-execute]]

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
const REQUEST_HANDLER_PATH = 'crates/missiond-daemon/src/handlers/knowledge/request.rs';
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
  --confirm-execute Reserved for future use. This checker explicitly refuses
                    workstation dispatch; with --confirm-execute the run
                    still stops at awaiting_execution and prints a notice
                    pointing the user to mission_request directly.
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

    const handlerAbs = path.resolve(opts.repo, REQUEST_HANDLER_PATH);
    try {
      const src = fs.readFileSync(handlerAbs, 'utf8');
      handlerResult = validateRequestHandlerSource(src, REQUEST_HANDLER_PATH);
      diagnostics.push(...handlerResult.diagnostics);
    } catch (err) {
      diagnostics.push({ file: REQUEST_HANDLER_PATH, message: `cannot read: ${err.message}` });
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
  const smokeObjective = compatRequested
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
    confirm_execute_refused: !!opts.confirmExecute,
    confirm_execute_notice: opts.confirmExecute
      ? '--confirm-execute is reserved; this checker still refuses to dispatch a workstation slot. Drive mission_request directly to execute.'
      : null,
    steps: [],
    cleanup: { requested: !!opts.cleanup, removed_path: null, kept_db_rows: 'directives, plans, board_tasks rows created during the live flow remain as audit records — only the request-local Lisp directory is cleaned up.' },
    legacy_compat_artifacts: null,
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
