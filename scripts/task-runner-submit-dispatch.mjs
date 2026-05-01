#!/usr/bin/env node

// MissionD task-runner dispatch submitter v0.
//
// Explicit bridge from task-runner-dispatch descriptors to the running
// missiond daemon IPC tools/call endpoint. Default mode is dry-run; --apply is
// required before any daemon call or lifecycle dispatch event write happens.

import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

import { runDispatch } from './task-runner-dispatch.mjs';
import { emitDispatchEventsForActions } from './task-runner-next-action.mjs';
import { projectWaveStateFromFiles } from './task-runner-wave-state.mjs';

const SUBMIT_SCHEMA = 'missiond.task-runner-dispatch-submit.v0';
const JSONRPC_VERSION = '2.0';

const usage = `Usage:
  node scripts/task-runner-submit-dispatch.mjs --manifest <manifest.lisp>
    [--lifecycle <task-lifecycle-events.lisp>] [--events-dir <task-events-dir>]
    [--receipts <receipts.lisp>]
    [--repo <repo-root>] [--max-parallel <n|all>] [--endpoint <ipc>]
    [--session-id <id>] [--actor-role <role>] [--allow-missing-briefs]
    [--request-id <request-id> --request-events-dir <dir>]
    [--blueprint <path>] [--allow-default-config]
    [--apply] [--json]
  node scripts/task-runner-submit-dispatch.mjs --dry-fixture [--json]

Dry-run by default: returns the dispatch descriptor and does not connect to
missiond. With --apply, submits each delegate call to daemon tools/call and
records lifecycle dispatch events only for successful submissions.
`;

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    manifest: null,
    lifecycle: null,
    eventsDir: null,
    receipts: null,
    repo: process.cwd(),
    maxParallel: 'all',
    endpoint: defaultEndpoint(),
    sessionId: defaultSessionId(),
    actorRole: 'orchestrator',
    requestId: null,
    requestEventsDir: null,
    blueprintPath: null,
    allowDefaultConfig: false,
    allowMissingBriefs: false,
    apply: false,
    json: false,
    dryFixture: false,
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
    } else if (arg === '--apply') {
      opts.apply = true;
    } else if (arg === '--allow-missing-briefs') {
      opts.allowMissingBriefs = true;
    } else if (arg === '--manifest') {
      opts.manifest = argv[++i] ?? fail('--manifest requires a value');
    } else if (arg.startsWith('--manifest=')) {
      opts.manifest = arg.slice('--manifest='.length);
    } else if (arg === '--lifecycle') {
      opts.lifecycle = argv[++i] ?? fail('--lifecycle requires a value');
    } else if (arg.startsWith('--lifecycle=')) {
      opts.lifecycle = arg.slice('--lifecycle='.length);
    } else if (arg === '--events-dir') {
      opts.eventsDir = argv[++i] ?? fail('--events-dir requires a value');
    } else if (arg.startsWith('--events-dir=')) {
      opts.eventsDir = arg.slice('--events-dir='.length);
    } else if (arg === '--receipts') {
      opts.receipts = argv[++i] ?? fail('--receipts requires a value');
    } else if (arg.startsWith('--receipts=')) {
      opts.receipts = arg.slice('--receipts='.length);
    } else if (arg === '--repo') {
      opts.repo = argv[++i] ?? fail('--repo requires a value');
    } else if (arg.startsWith('--repo=')) {
      opts.repo = arg.slice('--repo='.length);
    } else if (arg === '--max-parallel') {
      opts.maxParallel = argv[++i] ?? fail('--max-parallel requires a value');
    } else if (arg.startsWith('--max-parallel=')) {
      opts.maxParallel = arg.slice('--max-parallel='.length);
    } else if (arg === '--endpoint') {
      opts.endpoint = argv[++i] ?? fail('--endpoint requires a value');
    } else if (arg.startsWith('--endpoint=')) {
      opts.endpoint = arg.slice('--endpoint='.length);
    } else if (arg === '--session-id') {
      opts.sessionId = argv[++i] ?? fail('--session-id requires a value');
    } else if (arg.startsWith('--session-id=')) {
      opts.sessionId = arg.slice('--session-id='.length);
    } else if (arg === '--actor-role') {
      opts.actorRole = argv[++i] ?? fail('--actor-role requires a value');
    } else if (arg.startsWith('--actor-role=')) {
      opts.actorRole = arg.slice('--actor-role='.length);
    } else if (arg === '--request-id') {
      opts.requestId = argv[++i] ?? fail('--request-id requires a value');
    } else if (arg.startsWith('--request-id=')) {
      opts.requestId = arg.slice('--request-id='.length);
    } else if (arg === '--request-events-dir') {
      opts.requestEventsDir = argv[++i] ?? fail('--request-events-dir requires a value');
    } else if (arg.startsWith('--request-events-dir=')) {
      opts.requestEventsDir = arg.slice('--request-events-dir='.length);
    } else if (arg === '--blueprint') {
      opts.blueprintPath = argv[++i] ?? fail('--blueprint requires a value');
    } else if (arg.startsWith('--blueprint=')) {
      opts.blueprintPath = arg.slice('--blueprint='.length);
    } else if (arg === '--allow-default-config') {
      opts.allowDefaultConfig = true;
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

export async function submitDispatch({
  manifestPath,
  repoRoot = process.cwd(),
  lifecyclePath = null,
  eventsDirPath = null,
  receiptsPath = null,
  maxParallel = 'all',
  endpoint = defaultEndpoint(),
  sessionId = defaultSessionId(),
  actorRole = 'orchestrator',
  requestId = null,
  requestEventsDir = null,
  blueprintPath = null,
  allowDefaultConfig = false,
  allowMissingBriefs = false,
  apply = false,
  callTool = null,
  nowIso = isoNow(),
}) {
  const repo = path.resolve(repoRoot);
  const descriptor = runDispatch({
    manifestPath,
    repoRoot: repo,
    lifecyclePath,
    eventsDirPath,
    receiptsPath,
    maxParallel,
    actorRole,
    requestId,
    requestEventsDir,
    blueprintPath,
    allowDefaultConfig,
    allowMissingBriefs,
    emitDispatchEvents: false,
    nowIso,
  });
  const result = {
    ok: true,
    schema: SUBMIT_SCHEMA,
    mode: apply ? 'apply' : 'dry-run',
    endpoint: apply ? endpoint : null,
    session_id: apply ? sessionId : null,
    wave: descriptor.wave,
    manifest_path: descriptor.manifest_path,
    lifecycle_path: descriptor.lifecycle_path,
    events_dir_path: descriptor.events_dir_path,
    dispatch_status: descriptor.status,
    delegate_call_count: descriptor.delegate_call_count,
    submitted_count: 0,
    failed_count: 0,
    descriptor,
    submissions: [],
    appended_events: [],
    after_counts: null,
    after_running: null,
    after_dispatchable: null,
  };

  if (!apply || descriptor.delegate_calls.length === 0) return result;

  const caller = callTool ?? ((name, args) => callToolViaIpc({ endpoint, sessionId, name, arguments: args }));
  const successfulTaskIds = new Set();
  for (const call of descriptor.delegate_calls) {
    const submitted = await caller(call.target_tool, call.target_args);
    const ok = !submitted?.is_error;
    if (ok) successfulTaskIds.add(call.task_id);
    result.submissions.push({
      task_id: call.task_id,
      target_tool: call.target_tool,
      ok,
      tool_result: submitted,
    });
  }
  result.submitted_count = result.submissions.filter((s) => s.ok).length;
  result.failed_count = result.submissions.length - result.submitted_count;

  const successfulActions = descriptor.selected_actions.filter((action) =>
    successfulTaskIds.has(action.task_id),
  );
  if (successfulActions.length > 0) {
    result.appended_events = emitDispatchEventsForActions({
      actions: successfulActions,
      repoRoot: repo,
      lifecyclePath: descriptor.lifecycle_path,
      eventsDirPath: descriptor.events_dir_path,
      manifestPath: descriptor.manifest_path,
      actorRole,
      requestId,
      requestEventsDir: requestEventsDir ? path.resolve(repo, requestEventsDir) : null,
      nowIso,
      wave: descriptor.wave,
    });
    const after = projectWaveStateFromFiles({
      manifestPath,
      repoRoot: repo,
      lifecyclePath: descriptor.lifecycle_path,
      eventsDirPath: descriptor.events_dir_path,
      receiptsPath,
    });
    result.after_counts = after.counts;
    result.after_running = after.running;
    result.after_dispatchable = after.dispatchable;
  }
  return result;
}

export function callToolViaIpc({ endpoint, sessionId, name, arguments: args, timeoutMs = 60000 }) {
  return new Promise((resolve, reject) => {
    let settled = false;
    const request = {
      jsonrpc: JSONRPC_VERSION,
      method: 'tools/call',
      params: {
        name,
        arguments: args,
        _meta: { session_id: sessionId },
      },
      id: 1,
    };
    const socket = connect(endpoint);
    let data = '';
    socket.setEncoding('utf8');
    socket.setTimeout(timeoutMs, () => {
      if (settled) return;
      settled = true;
      socket.destroy();
      reject(new Error(`IPC call timed out after ${timeoutMs}ms`));
    });
    socket.on('data', (chunk) => {
      data += chunk;
      const newline = data.indexOf('\n');
      if (newline >= 0) {
        const line = data.slice(0, newline).trim();
        if (settled) return;
        settled = true;
        socket.end();
        try {
          const response = JSON.parse(line);
          if (response.error) {
            resolve(errorToolResult(response.error.message ?? 'IPC error'));
          } else {
            resolve(response.result ?? errorToolResult('IPC response missing result'));
          }
        } catch (err) {
          reject(err);
        }
      }
    });
    socket.on('error', (err) => {
      if (settled) return;
      settled = true;
      reject(err);
    });
    socket.on('connect', () => {
      socket.write(`${JSON.stringify(request)}\n`);
    });
  });
}

function connect(endpoint) {
  if (endpoint.includes(':') && !endpoint.startsWith('/')) {
    const [host, portText] = endpoint.split(':');
    const port = Number.parseInt(portText, 10);
    return net.createConnection({ host, port });
  }
  return net.createConnection(endpoint);
}

function errorToolResult(message) {
  return {
    content: [{ type: 'text', text: JSON.stringify({ error: message }) }],
    is_error: true,
  };
}

function defaultEndpoint() {
  if (process.env.MISSION_IPC_ENDPOINT) return process.env.MISSION_IPC_ENDPOINT;
  if (process.env.MISSION_IPC_SOCKET) return process.env.MISSION_IPC_SOCKET;
  return path.join(os.homedir(), '.missiond', 'missiond.sock');
}

function defaultSessionId() {
  return process.env.CLAUDE_SESSION_ID ?? process.env.SESSION_ID ?? `task-runner-${process.pid}`;
}

function isoNow() {
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    submitDispatchFixtures().then(
      (result) => {
        if (opts.json) console.log(JSON.stringify(result, null, 2));
        else console.log(`task-runner-submit-dispatch fixtures OK (${result.cases} cases)`);
      },
      (err) => {
        console.error(err.stack || err.message);
        process.exit(1);
      },
    );
    return;
  }
  if (!opts.manifest) fail('--manifest is required');
  submitDispatch({
    manifestPath: opts.manifest,
    repoRoot: opts.repo,
    lifecyclePath: opts.lifecycle,
    eventsDirPath: opts.eventsDir,
    receiptsPath: opts.receipts,
    maxParallel: opts.maxParallel,
    endpoint: opts.endpoint,
    sessionId: opts.sessionId,
    actorRole: opts.actorRole,
    requestId: opts.requestId,
    requestEventsDir: opts.requestEventsDir,
    blueprintPath: opts.blueprintPath,
    allowDefaultConfig: opts.allowDefaultConfig,
    allowMissingBriefs: opts.allowMissingBriefs,
    apply: opts.apply,
  }).then(
    (result) => {
      if (opts.json) {
        console.log(JSON.stringify(result, null, 2));
      } else {
        console.log(
          `task-runner-submit-dispatch OK (${result.wave}): ` +
            `${result.mode}, ${result.submitted_count}/${result.delegate_call_count} submitted`,
        );
      }
    },
    (err) => {
      console.error(`task-runner-submit-dispatch: ${err?.message ?? String(err)}`);
      process.exit(1);
    },
  );
}

async function submitDispatchFixtures() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-submit-dispatch-'));
  try {
    const manifestPath = path.join(tmp, '.missiond/tasks/wave99/manifest.lisp');
    const briefDir = path.join(tmp, '.missiond/claudecode');
    fs.mkdirSync(path.dirname(manifestPath), { recursive: true });
    fs.mkdirSync(briefDir, { recursive: true });
    const blueprintPath = path.join(tmp, '.missiond/v3/missiond-blueprint.lisp');
    fs.mkdirSync(path.dirname(blueprintPath), { recursive: true });
    fs.writeFileSync(blueprintPath, fixtureBlueprint());
    fs.writeFileSync(manifestPath, fixtureManifest());
    fs.writeFileSync(path.join(briefDir, 'wave99-01-alpha.md'), '# alpha\n');
    fs.writeFileSync(path.join(briefDir, 'wave99-02-beta.md'), '# beta\n');

    const dry = await submitDispatch({
      manifestPath,
      repoRoot: tmp,
      maxParallel: 1,
      apply: false,
      nowIso: '2026-04-28T00:00:00Z',
    });
    assert(dry.mode === 'dry-run', 'default fixture should be dry-run');
    assert(dry.submitted_count === 0, 'dry-run should submit nothing');
    assert(dry.delegate_call_count === 1, 'dry-run should still expose one delegate call');
    assert(
      dry.descriptor.runtime_projection.default_model_profile === 'submit-fixture-opus-4-7',
      'submit dry-run should project dispatch defaults from V3 workstation-config',
    );
    assert(
      dry.descriptor.delegate_calls[0].target_args.timeout_secs === 3660,
      'submit dry-run should use V3 default timeout in delegate payload',
    );

    const calls = [];
    const applied = await submitDispatch({
      manifestPath,
      repoRoot: tmp,
      maxParallel: 1,
      apply: true,
      requestId: 'req-wave99-submit',
      requestEventsDir: '.missiond/requests/req-wave99-submit/events',
      callTool: async (name, args) => {
        calls.push({ name, args });
        return {
          content: [{ type: 'text', text: JSON.stringify({ task_id: 'board-1', status: 'queued' }) }],
        };
      },
      nowIso: '2026-04-28T00:01:00Z',
    });
    assert(calls.length === 1, 'apply should call one delegate payload');
    assert(applied.submitted_count === 1, 'apply should record one successful submission');
    assert(applied.appended_events.length === 1, 'successful submission should append dispatch event');
    assert(
      applied.appended_events[0].request_event_path?.endsWith('000001.event.lisp'),
      'successful submission should project request-local dispatch event when request args are supplied',
    );
    assert(
      applied.appended_events[0].event_file?.endsWith('.event.lisp'),
      'successful submission should also write task-scoped one-event file via auto-detected events-dir',
    );
    assert(
      applied.events_dir_path === '.missiond/tasks/wave99/events',
      'submit-dispatch should expose the resolved task-scoped events_dir_path',
    );
    assert(applied.after_running.length === 1, 'after projection should show one running task');

    const partial = await submitDispatch({
      manifestPath,
      repoRoot: tmp,
      maxParallel: 'all',
      apply: true,
      callTool: async (name, args) => ({
        content: [{ type: 'text', text: JSON.stringify({ error: `${name}:${args.objective}` }) }],
        is_error: true,
      }),
      nowIso: '2026-04-28T00:02:00Z',
    });
    assert(partial.failed_count >= 1, 'failed submissions should be counted');
    assert(partial.appended_events.length === 0, 'failed submissions should not append dispatch events');

    return { ok: true, cases: 3 };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function fixtureManifest() {
  return `(task-runner-manifest wave99-submit
  :schema "missiond.task-runner-manifest.v2"
  :wave wave99
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave99-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  (node :task_id wave99-01-alpha
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 15
        :heartbeat_minutes 5
        :write_scope ["scripts/wave99-alpha.mjs"])
  (node :task_id wave99-02-beta
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 20
        :heartbeat_minutes 5
        :write_scope ["scripts/wave99-beta.mjs"]))\n`;
}

function fixtureBlueprint() {
  return `(missiond-blueprint
  (workstation-config
    (slot-template coder
      :role coder
      :default-model-profile submit-fixture-opus-4-7)
    (timeout-policy boardtask-dispatch
      :default_secs 3660
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    (ttl-policy dynamic-slot
      :default_secs 14400
      :min_secs 300
      :max_secs 28800
      :default_extend_secs 3600
      :max_extend_secs 3600)))`;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
