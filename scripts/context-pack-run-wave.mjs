#!/usr/bin/env node

// V3 context-pack wave runner.
//
// This is the thin orchestration boundary for two-stage ClaudeCode work:
//   context-pack.lisp -> materialized task-runner wave -> prepared briefs ->
//   dispatch descriptor, submit dry-run, or explicit daemon apply.
//
// The context-pack remains the SSOT. This script composes the existing
// materialize/prepare/dispatch surfaces and keeps real worker submission
// behind --apply: never calls the daemon or spawns workers unless --apply.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

import { materializeContextPackWave } from './context-pack-materialize-wave.mjs';
import { prepareWave } from './prepare-task-runner-wave.mjs';
import { runDispatch } from './task-runner-dispatch.mjs';
import { submitDispatch } from './task-runner-submit-dispatch.mjs';
import { loadWorkstationRuntimeConfigForRepo } from './lib/v3_workstation_runtime.mjs';

const RUNNER_SCHEMA = 'missiond.context-pack-run-wave.v0';

const usage = `Usage:
  node scripts/context-pack-run-wave.mjs --context-pack <context-pack.lisp>
    [--repo <repo>] [--wave <wave>] [--task-prefix <prefix>]
    [--estimated-minutes <n>] [--heartbeat-minutes <n>] [--timeout-secs <n>]
    [--model-profile <profile>] [--blueprint <path>] [--allow-default-config]
    [--max-parallel <n|all>] [--force] [--dry-run] [--skip-prepare]
    [--skip-ledger-init] [--no-dispatch] [--submit] [--apply] [--allow-missing-briefs]
    [--emit-dispatch-events] [--endpoint <ipc>] [--session-id <id>]
    [--request-id <id> --request-events-dir <dir>]
    [--lifecycle <path>] [--events-dir <dir>] [--receipts <path>] [--json]
  node scripts/context-pack-run-wave.mjs --dry-fixture [--json]

Runs the V3 two-stage context-pack implementation boundary. Default mode writes
the materialized manifest/contracts, prepares thin briefs/report skeletons,
then returns a read-only task-runner dispatch descriptor. It never calls the
daemon or spawns workers unless --apply is supplied.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    runFixtures().then(
      (result) => {
        if (opts.json) console.log(JSON.stringify(result, null, 2));
        else console.log(`context-pack-run-wave fixtures OK (${result.cases} cases)`);
      },
      (err) => {
        console.error(err?.stack ?? err?.message ?? String(err));
        process.exit(1);
      },
    );
    return;
  }
  if (!opts.contextPack) fail('--context-pack is required');
  runContextPackWave({
    contextPackPath: opts.contextPack,
    repoRoot: opts.repo,
    wave: opts.wave,
    taskPrefix: opts.taskPrefix,
    estimatedMinutes: opts.estimatedMinutes,
    heartbeatMinutes: opts.heartbeatMinutes,
    timeoutSecs: opts.timeoutSecs,
    modelProfile: opts.modelProfile,
    blueprintPath: opts.blueprintPath,
    allowDefaultConfig: opts.allowDefaultConfig,
    maxParallel: opts.maxParallel,
    force: opts.force,
    dryRun: opts.dryRun,
    skipPrepare: opts.skipPrepare,
    noDispatch: opts.noDispatch,
    submit: opts.submit,
    apply: opts.apply,
    allowMissingBriefs: opts.allowMissingBriefs,
    emitDispatchEvents: opts.emitDispatchEvents,
    endpoint: opts.endpoint,
    sessionId: opts.sessionId,
    requestId: opts.requestId,
    requestEventsDir: opts.requestEventsDir,
    lifecyclePath: opts.lifecycle,
    eventsDirPath: opts.eventsDir,
    receiptsPath: opts.receipts,
  }).then(
    (result) => {
      if (opts.json) {
        console.log(JSON.stringify(result, null, 2));
      } else {
        console.log(
          `context-pack-run-wave OK (${result.wave}): ${result.mode}, ` +
            `${result.materialize.task_count} task(s)` +
            (result.dispatch ? `, dispatch=${result.dispatch.status ?? result.dispatch.mode}` : ''),
        );
      }
    },
    (err) => {
      console.error(`context-pack-run-wave: ${err?.message ?? String(err)}`);
      process.exit(1);
    },
  );
}

function parseArgs(args) {
  const opts = {
    contextPack: null,
    repo: process.cwd(),
    wave: null,
    taskPrefix: null,
    estimatedMinutes: null,
    heartbeatMinutes: null,
    timeoutSecs: null,
    modelProfile: null,
    blueprintPath: null,
    allowDefaultConfig: false,
    maxParallel: null,
    force: false,
    dryRun: false,
    skipPrepare: false,
    skipLedgerInit: false,
    noDispatch: false,
    submit: false,
    apply: false,
    allowMissingBriefs: false,
    emitDispatchEvents: false,
    endpoint: null,
    sessionId: null,
    requestId: null,
    requestEventsDir: null,
    lifecycle: null,
    eventsDir: null,
    receipts: null,
    json: false,
    dryFixture: false,
  };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') opts.json = true;
    else if (arg === '--dry-fixture') opts.dryFixture = true;
    else if (arg === '--allow-default-config') opts.allowDefaultConfig = true;
    else if (arg === '--force') opts.force = true;
    else if (arg === '--dry-run') opts.dryRun = true;
    else if (arg === '--skip-prepare') opts.skipPrepare = true;
    else if (arg === '--skip-ledger-init') opts.skipLedgerInit = true;
    else if (arg === '--no-dispatch') opts.noDispatch = true;
    else if (arg === '--submit') opts.submit = true;
    else if (arg === '--apply') opts.apply = true;
    else if (arg === '--allow-missing-briefs') opts.allowMissingBriefs = true;
    else if (arg === '--emit-dispatch-events') opts.emitDispatchEvents = true;
    else if (arg === '--context-pack') opts.contextPack = need(args, ++i, arg);
    else if (arg.startsWith('--context-pack=')) opts.contextPack = arg.slice('--context-pack='.length);
    else if (arg === '--repo') opts.repo = need(args, ++i, arg);
    else if (arg.startsWith('--repo=')) opts.repo = arg.slice('--repo='.length);
    else if (arg === '--wave') opts.wave = need(args, ++i, arg);
    else if (arg.startsWith('--wave=')) opts.wave = arg.slice('--wave='.length);
    else if (arg === '--task-prefix') opts.taskPrefix = need(args, ++i, arg);
    else if (arg.startsWith('--task-prefix=')) opts.taskPrefix = arg.slice('--task-prefix='.length);
    else if (arg === '--estimated-minutes') opts.estimatedMinutes = parsePositiveInt(need(args, ++i, arg), arg);
    else if (arg.startsWith('--estimated-minutes=')) opts.estimatedMinutes = parsePositiveInt(arg.slice('--estimated-minutes='.length), '--estimated-minutes');
    else if (arg === '--heartbeat-minutes') opts.heartbeatMinutes = parsePositiveInt(need(args, ++i, arg), arg);
    else if (arg.startsWith('--heartbeat-minutes=')) opts.heartbeatMinutes = parsePositiveInt(arg.slice('--heartbeat-minutes='.length), '--heartbeat-minutes');
    else if (arg === '--timeout-secs') opts.timeoutSecs = parsePositiveInt(need(args, ++i, arg), arg);
    else if (arg.startsWith('--timeout-secs=')) opts.timeoutSecs = parsePositiveInt(arg.slice('--timeout-secs='.length), '--timeout-secs');
    else if (arg === '--model-profile') opts.modelProfile = need(args, ++i, arg);
    else if (arg.startsWith('--model-profile=')) opts.modelProfile = arg.slice('--model-profile='.length);
    else if (arg === '--blueprint') opts.blueprintPath = need(args, ++i, arg);
    else if (arg.startsWith('--blueprint=')) opts.blueprintPath = arg.slice('--blueprint='.length);
    else if (arg === '--max-parallel') opts.maxParallel = parseMaxParallel(need(args, ++i, arg), arg);
    else if (arg.startsWith('--max-parallel=')) opts.maxParallel = parseMaxParallel(arg.slice('--max-parallel='.length), '--max-parallel');
    else if (arg === '--endpoint') opts.endpoint = need(args, ++i, arg);
    else if (arg.startsWith('--endpoint=')) opts.endpoint = arg.slice('--endpoint='.length);
    else if (arg === '--session-id') opts.sessionId = need(args, ++i, arg);
    else if (arg.startsWith('--session-id=')) opts.sessionId = arg.slice('--session-id='.length);
    else if (arg === '--request-id') opts.requestId = need(args, ++i, arg);
    else if (arg.startsWith('--request-id=')) opts.requestId = arg.slice('--request-id='.length);
    else if (arg === '--request-events-dir') opts.requestEventsDir = need(args, ++i, arg);
    else if (arg.startsWith('--request-events-dir=')) opts.requestEventsDir = arg.slice('--request-events-dir='.length);
    else if (arg === '--lifecycle') opts.lifecycle = need(args, ++i, arg);
    else if (arg.startsWith('--lifecycle=')) opts.lifecycle = arg.slice('--lifecycle='.length);
    else if (arg === '--events-dir') opts.eventsDir = need(args, ++i, arg);
    else if (arg.startsWith('--events-dir=')) opts.eventsDir = arg.slice('--events-dir='.length);
    else if (arg === '--receipts') opts.receipts = need(args, ++i, arg);
    else if (arg.startsWith('--receipts=')) opts.receipts = arg.slice('--receipts='.length);
    else fail(`unknown argument: ${arg}`);
  }
  if (opts.apply) opts.submit = true;
  if (opts.emitDispatchEvents && opts.submit) {
    fail('--emit-dispatch-events is only valid for descriptor mode; submit/apply records events after successful submissions');
  }
  return opts;
}

export async function runContextPackWave({
  contextPackPath,
  repoRoot = process.cwd(),
  wave = null,
  taskPrefix = null,
  estimatedMinutes = null,
  heartbeatMinutes = null,
  timeoutSecs = null,
  modelProfile = null,
  blueprintPath = null,
  allowDefaultConfig = false,
  maxParallel = null,
  force = false,
  dryRun = false,
  skipPrepare = false,
  skipLedgerInit = false,
  noDispatch = false,
  submit = false,
  apply = false,
  allowMissingBriefs = false,
  emitDispatchEvents = false,
  endpoint = null,
  sessionId = null,
  requestId = null,
  requestEventsDir = null,
  lifecyclePath = null,
  eventsDirPath = null,
  receiptsPath = null,
  nowIso = isoNow(),
} = {}) {
  if (!contextPackPath) throw new Error('contextPackPath is required');
  const repo = path.resolve(repoRoot);
  const runtimeConfig = loadWorkstationRuntimeConfigForRepo(repo, {
    blueprintPath,
    allowDefaultFallback: allowDefaultConfig,
  });
  const effectiveMaxParallel = runtimeConfig.contextPackMaxParallel(maxParallel);
  const materialize = materializeContextPackWave({
    contextPackPath,
    repoRoot: repo,
    wave,
    taskPrefix,
    estimatedMinutes: estimatedMinutes ?? undefined,
    heartbeatMinutes: heartbeatMinutes ?? undefined,
    timeoutSecs,
    modelProfile,
    blueprintPath,
    allowDefaultConfig,
    dryRun,
    force,
    nowIso,
  });
  const manifestPath = path.join(repo, '.missiond', 'tasks', materialize.wave, 'manifest.lisp');
  const manifestRel = repoPath(repo, manifestPath);

  if (dryRun) {
    return {
      ok: true,
      schema: RUNNER_SCHEMA,
      mode: 'dry-run',
      wave: materialize.wave,
      context_pack_path: materialize.context_pack_path,
      materialize,
      prepare: null,
      dispatch: null,
      runtime_projection: {
        config_source: runtimeConfig.source,
        max_parallel: effectiveMaxParallel,
      },
      next_commands: nextCommands({
        contextPackPath: materialize.context_pack_path,
        manifestPath: manifestRel,
        maxParallel: effectiveMaxParallel,
      }),
    };
  }

  const prepare = skipPrepare
    ? null
    : prepareWaveWithLedgers({
        repo,
        wave: materialize.wave,
        manifestPath,
        force,
        nowIso,
        skipLedgerInit,
      });

  let dispatch = null;
  let mode = 'prepared';
  if (!noDispatch) {
    if (submit || apply) {
      dispatch = await submitDispatch({
        manifestPath,
        repoRoot: repo,
        lifecyclePath,
        eventsDirPath,
        receiptsPath,
        maxParallel: effectiveMaxParallel,
        endpoint: endpoint ?? undefined,
        sessionId: sessionId ?? undefined,
        actorRole: 'context-pack-run-wave',
        requestId,
        requestEventsDir,
        blueprintPath,
        allowDefaultConfig,
        allowMissingBriefs,
        apply,
      });
      mode = apply ? 'apply' : 'submit-dry-run';
    } else {
      dispatch = runDispatch({
        manifestPath,
        repoRoot: repo,
        lifecyclePath,
        eventsDirPath,
        receiptsPath,
        maxParallel: effectiveMaxParallel,
        actorRole: 'context-pack-run-wave',
        requestId,
        requestEventsDir,
        blueprintPath,
        allowDefaultConfig,
        allowMissingBriefs,
        emitDispatchEvents,
        nowIso,
      });
      mode = emitDispatchEvents ? 'descriptor-with-dispatch-events' : 'descriptor';
    }
  }

  return {
    ok: true,
    schema: RUNNER_SCHEMA,
    mode,
    wave: materialize.wave,
    context_pack_path: materialize.context_pack_path,
    manifest_path: manifestRel,
    materialize,
    prepare,
    ledger_init: prepare?.ledger_init ?? null,
    dispatch,
    runtime_projection: {
      config_source: runtimeConfig.source,
      max_parallel: effectiveMaxParallel,
    },
    next_commands: nextCommands({
      contextPackPath: materialize.context_pack_path,
      manifestPath: manifestRel,
      maxParallel: effectiveMaxParallel,
    }),
  };
}

function nextCommands({ contextPackPath, manifestPath, maxParallel }) {
  return [
    `node scripts/context-pack-run-wave.mjs --context-pack ${contextPackPath} --max-parallel ${maxParallel}`,
    `node scripts/context-pack-run-wave.mjs --context-pack ${contextPackPath} --max-parallel ${maxParallel} --apply`,
    `node scripts/task-runner-dispatch.mjs --manifest ${manifestPath} --max-parallel ${maxParallel} --json`,
  ];
}

function prepareWaveWithLedgers({ repo, wave, manifestPath, force, nowIso, skipLedgerInit }) {
  const ledgerInit = skipLedgerInit
    ? { skipped: true, shared_memory: null, session_trace: null }
    : ensureWaveLedgers({ repo, wave, nowIso });
  const result = prepareWave({
    manifestPath,
    cwd: repo,
    dryRun: false,
    force,
    nowIso,
  });
  return { ...result, ledger_init: ledgerInit };
}

function ensureWaveLedgers({ repo, wave, nowIso }) {
  const taskDir = path.join(repo, '.missiond', 'tasks', wave);
  fs.mkdirSync(taskDir, { recursive: true });
  const sharedMemoryPath = path.join(taskDir, 'shared-memory.lisp');
  const sessionTracePath = path.join(taskDir, 'session-trace.lisp');
  return {
    skipped: false,
    shared_memory: writeCreateOnly(
      sharedMemoryPath,
      `(shared-memory ${wave}
  :schema "missiond.shared-memory.v1"
  :wave ${wave}
  :created-at "${nowIso}"
  :sequence 0)
`,
      repo,
    ),
    session_trace: writeCreateOnly(
      sessionTracePath,
      `(session-trace ${wave}
  :schema "missiond.session-trace.v1"
  :wave ${wave}
  :created-at "${nowIso}"
  :sequence 0)
`,
      repo,
    ),
  };
}

function writeCreateOnly(file, source, repo) {
  if (fs.existsSync(file)) return { path: repoPath(repo, file), action: 'skipped-existing' };
  fs.writeFileSync(file, source);
  return { path: repoPath(repo, file), action: 'created', bytes: Buffer.byteLength(source) };
}

async function runFixtures() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-context-pack-run-wave-'));
  try {
    const packPath = path.join(tmp, '.missiond/tasks/wave99/context-pack.lisp');
    fs.mkdirSync(path.dirname(packPath), { recursive: true });
    fs.writeFileSync(packPath, fixtureContextPack());
    const blueprintPath = path.join(tmp, '.missiond/v3/missiond-blueprint.lisp');
    fs.mkdirSync(path.dirname(blueprintPath), { recursive: true });
    fs.writeFileSync(blueprintPath, fixtureBlueprint());
    const descriptor = await runContextPackWave({
      contextPackPath: packPath,
      repoRoot: tmp,
      nowIso: '2026-04-29T00:00:00Z',
    });
    assert(descriptor.mode === 'descriptor', 'default mode should return dispatch descriptor');
    assert(descriptor.materialize.task_count === 2, 'expected two materialized tasks');
    assert(descriptor.runtime_projection.max_parallel === '1', 'expected V3 projected max parallel');
    assert(descriptor.ledger_init.shared_memory.action === 'created', 'runner should create missing shared-memory ledger');
    assert(descriptor.ledger_init.session_trace.action === 'created', 'runner should create missing session-trace ledger');
    assert(descriptor.prepare.briefsWritten === 2, 'expected two rendered briefs');
    assert(descriptor.prepare.skeletonsWritten === 2, 'expected two report skeletons');
    assert(descriptor.dispatch.status === 'ready_to_delegate', 'expected ready dispatch status');
    assert(descriptor.dispatch.delegate_call_count === 1, 'maxParallel=1 should select one task');
    const objective = descriptor.dispatch.delegate_calls[0].target_args.objective;
    for (const literal of [
      'Model profile: runner-fixture-opus-4-7',
      'Timeout seconds: 3660',
      'Context pack: .missiond/tasks/wave99/context-pack.lisp',
      'BoardTask ID: assigned by mission_task_delegate',
    ]) {
      assert(objective.includes(literal), `objective should include ${literal}`);
    }

    const override = await runContextPackWave({
      contextPackPath: packPath,
      repoRoot: tmp,
      maxParallel: '2',
      force: false,
      nowIso: '2026-04-29T00:00:00Z',
    });
    assert(override.runtime_projection.max_parallel === '2', 'explicit maxParallel should override V3 default');
    assert(override.ledger_init.shared_memory.action === 'skipped-existing', 'runner should not rewrite existing shared-memory ledger');
    assert(override.dispatch.delegate_call_count === 2, 'explicit maxParallel=2 should select two tasks');

    const submitDry = await runContextPackWave({
      contextPackPath: packPath,
      repoRoot: tmp,
      submit: true,
      force: false,
      nowIso: '2026-04-29T00:00:00Z',
    });
    assert(submitDry.mode === 'submit-dry-run', 'submit without apply should stay dry-run');
    assert(submitDry.dispatch.mode === 'dry-run', 'submit descriptor should not call daemon');
    assert(submitDry.dispatch.delegate_call_count === 1, 'submit dry-run should preserve delegate calls');

    const dry = await runContextPackWave({
      contextPackPath: packPath,
      repoRoot: tmp,
      wave: 'wave100',
      taskPrefix: 'wave100',
      dryRun: true,
      nowIso: '2026-04-29T00:02:00Z',
    });
    assert(dry.mode === 'dry-run', 'dry-run should report dry-run mode');
    assert(!fs.existsSync(path.join(tmp, '.missiond/tasks/wave100/manifest.lisp')), 'dry-run should not write manifest');

    return { ok: true, cases: 4 };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function fixtureContextPack() {
  return `(context-pack wave99-context-pack
  :schema "missiond.context-pack.v1"
  :wave wave99
  :purpose "Run dispatchable code-worker shards."
  :write-model append-only
  :sequence 3
  (shard-proposal :id s1 :agent context-a :seq 1 :at "2026-04-29T00:00:00Z" :shard alpha :owner worker-a :summary "Alpha summary." :write-scope ["scripts/alpha.mjs"] :must-not-touch ["packages/**"] :acceptance ["node scripts/check-v3-code-isomorphism-complete.mjs"])
  (shard-proposal :id s2 :agent context-b :seq 2 :at "2026-04-29T00:00:01Z" :shard beta :owner worker-b :summary "Beta summary." :write-scope ["scripts/beta.mjs"] :must-not-touch ["scripts/alpha.mjs"] :acceptance ["git diff --check"])
  (integration-plan :id i3 :agent integrator :seq 3 :at "2026-04-29T00:00:02Z" :summary "accept both" :accepted-shards [alpha beta] :dispatch-groups [(group :id A :shards [alpha]) (group :id B :shards [beta])]))`;
}

function fixtureBlueprint() {
  return `(missiond-blueprint
  (workstation-config
    (slot-template coder
      :role coder
      :default-model-profile runner-fixture-opus-4-7)
    (timeout-policy boardtask-dispatch
      :default_secs 3660
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    (timeout-policy claudecode-swarm
      :default_secs 600
      :min_secs 60
      :max_secs 7200)
    (timeout-policy pty-send-blocking
      :default_secs 300
      :min_secs 1
      :max_secs 7200)
    (dispatch-policy context-pack-run-wave
      :default_max_parallel 1
      :min_parallel 1
      :max_parallel 4)
    (ttl-policy dynamic-slot
      :default_secs 14400
      :min_secs 300
      :max_secs 28800
      :default_extend_secs 3600
      :max_extend_secs 3600)))`;
}

function parseMaxParallel(value, flag) {
  if (value === 'all') return value;
  return String(parsePositiveInt(value, flag));
}

function parsePositiveInt(value, flag) {
  if (!/^[1-9][0-9]*$/.test(String(value))) fail(`${flag} requires a positive integer`);
  return Number.parseInt(value, 10);
}

function repoPath(repo, file) {
  return path.relative(repo, file).split(path.sep).join('/');
}

function need(args, index, flag) {
  const value = args[index];
  if (!value || value.startsWith('--')) fail(`${flag} requires a value`);
  return value;
}

function isoNow() {
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function fail(message) {
  console.error(`${message}\n\n${usage}`);
  process.exit(2);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
