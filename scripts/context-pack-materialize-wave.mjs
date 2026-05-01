#!/usr/bin/env node

// Materialize a V3 context-pack integration-plan into task-runner Lisp.
//
// The context-pack remains the SSOT for two-stage parallel work. This script
// only projects the latest mapped integration-plan into:
//   - .missiond/tasks/<wave>/manifest.lisp
//   - .missiond/tasks/<wave>/<task-id>.lisp task contracts
//
// It does not dispatch workers. Downstream orchestration still runs
// prepare-task-runner-wave.mjs, task-runner-dispatch.mjs, or
// task-runner-submit-dispatch.mjs.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

import { compileContextPackSource } from './context-pack-compile-shards.mjs';
import { parseLisp } from './lib/missiond_lisp.mjs';
import {
  DEFAULT_MODEL_PROFILE,
  loadWorkstationRuntimeConfigForRepo,
} from './lib/v3_workstation_runtime.mjs';
import { projectManifest, validateManifestObject } from './check-task-runner-manifest.mjs';

const SCRIPT = 'scripts/context-pack-materialize-wave.mjs';
const MANIFEST_SCHEMA = 'missiond.task-runner-manifest.v2';
const TASK_SCHEMA = 'missiond.task-contract.v1';
const DEFAULT_ESTIMATED_MINUTES = 30;
const DEFAULT_HEARTBEAT_MINUTES = 10;

const usage = `Usage:
  node scripts/context-pack-materialize-wave.mjs --context-pack <context-pack.lisp>
    [--out-dir <repo>] [--wave <wave>] [--task-prefix <prefix>]
    [--estimated-minutes <n>] [--heartbeat-minutes <n>] [--timeout-secs <n>]
    [--model-profile <profile>] [--blueprint <path>] [--allow-default-config]
    [--dry-run] [--force] [--json]
  node scripts/context-pack-materialize-wave.mjs --dry-fixture [--json]

Projects a mapped context-pack integration-plan into task-runner manifest +
task contracts. It refuses names-only dispatch groups because code-worker
waves need explicit shard-to-group mapping and disjoint write scopes.

Default mode writes only missing files or byte-identical files. Use --force to
replace existing generated files with different bytes.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    const result = runFixtures();
    if (opts.json) console.log(JSON.stringify(result, null, 2));
    else console.log(`context-pack materialize fixtures OK (${result.cases} cases)`);
    return;
  }
  if (!opts.contextPack) fail('--context-pack is required');
  const result = materializeContextPackWave({
    contextPackPath: opts.contextPack,
    repoRoot: opts.outDir ?? process.cwd(),
    wave: opts.wave,
    taskPrefix: opts.taskPrefix,
    estimatedMinutes: opts.estimatedMinutes,
    heartbeatMinutes: opts.heartbeatMinutes,
    timeoutSecs: opts.timeoutSecs,
    modelProfile: opts.modelProfile,
    blueprintPath: opts.blueprintPath,
    allowDefaultConfig: opts.allowDefaultConfig,
    dryRun: opts.dryRun,
    force: opts.force,
    nowIso: isoNow(),
  });
  if (opts.json) console.log(JSON.stringify(result, null, 2));
  else {
    console.log(
      `context-pack materialize OK (${result.wave}): ` +
        `${result.tasks.length} task(s), manifest ${result.outputs.manifest.action}` +
        (result.dry_run ? ' (dry-run)' : ''),
    );
  }
}

function parseArgs(args) {
  const opts = {
    contextPack: null,
    outDir: null,
    wave: null,
    taskPrefix: null,
    estimatedMinutes: DEFAULT_ESTIMATED_MINUTES,
    heartbeatMinutes: DEFAULT_HEARTBEAT_MINUTES,
    timeoutSecs: null,
    modelProfile: null,
    blueprintPath: null,
    allowDefaultConfig: false,
    dryRun: false,
    force: false,
    json: false,
    dryFixture: false,
  };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') opts.json = true;
    else if (arg === '--dry-run') opts.dryRun = true;
    else if (arg === '--force') opts.force = true;
    else if (arg === '--dry-fixture') opts.dryFixture = true;
    else if (arg === '--context-pack') opts.contextPack = need(args, ++i, arg);
    else if (arg.startsWith('--context-pack=')) opts.contextPack = arg.slice('--context-pack='.length);
    else if (arg === '--out-dir') opts.outDir = need(args, ++i, arg);
    else if (arg.startsWith('--out-dir=')) opts.outDir = arg.slice('--out-dir='.length);
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
    else if (arg === '--allow-default-config') opts.allowDefaultConfig = true;
    else fail(`unknown argument: ${arg}`);
  }
  return opts;
}

export function materializeContextPackWave({
  contextPackPath,
  repoRoot = process.cwd(),
  wave = null,
  taskPrefix = null,
  estimatedMinutes = DEFAULT_ESTIMATED_MINUTES,
  heartbeatMinutes = DEFAULT_HEARTBEAT_MINUTES,
  timeoutSecs = null,
  modelProfile = null,
  blueprintPath = null,
  allowDefaultConfig = false,
  dryRun = false,
  force = false,
  nowIso = isoNow(),
}) {
  const repo = path.resolve(repoRoot);
  const runtimeConfig = loadWorkstationRuntimeConfigForRepo(repo, {
    blueprintPath,
    allowDefaultFallback: allowDefaultConfig,
  });
  const effectiveModelProfile =
    modelProfile ??
    runtimeConfig.defaultModelProfileForTemplate('coder') ??
    DEFAULT_MODEL_PROFILE;
  const effectiveTimeoutSecs = runtimeConfig.clampTimeoutSecs(timeoutSecs);
  validateToken(effectiveModelProfile, 'model profile');

  const contextPackAbs = path.resolve(process.cwd(), contextPackPath);
  const contextPackRel = repoRelative(repo, contextPackAbs, 'context-pack');
  const source = fs.readFileSync(contextPackAbs, 'utf8');
  const compiled = compileContextPackSource(source, contextPackAbs);
  if (compiled.group_mode !== 'mapped' || compiled.dispatchable_groups.length === 0) {
    throw new Error(
      'context-pack integration-plan must use mapped dispatch groups: ' +
        ':dispatch-groups [(group :id A :shards [...]) ...]',
    );
  }

  const outputWave = wave ?? compiled.wave;
  validateToken(outputWave, 'wave');
  const prefix = taskPrefix ?? outputWave;
  validateToken(prefix, 'task prefix');

  const tasks = buildTasks({
    compiled,
    wave: outputWave,
    taskPrefix: prefix,
    contextPackRel,
    estimatedMinutes,
    heartbeatMinutes,
    timeoutSecs: effectiveTimeoutSecs,
    modelProfile: effectiveModelProfile,
  });
  const manifestSource = renderManifest({
    wave: outputWave,
    contextPackRel,
    nowIso,
    modelProfile: effectiveModelProfile,
    estimatedMinutes,
    heartbeatMinutes,
    timeoutSecs: effectiveTimeoutSecs,
    tasks,
  });
  validateManifestSource(manifestSource);

  const taskSources = tasks.map((task) => ({
    task,
    source: renderTaskContract(task),
  }));
  validateTaskSources(taskSources);

  const taskDir = path.join(repo, '.missiond', 'tasks', outputWave);
  const manifestPath = path.join(taskDir, 'manifest.lisp');
  const outputs = {
    manifest: dryRun
      ? plannedWrite(manifestPath, manifestSource)
      : writeFileChecked(manifestPath, manifestSource, { force }),
    contracts: [],
  };
  for (const item of taskSources) {
    const file = path.join(taskDir, `${item.task.task_id}.lisp`);
    outputs.contracts.push(
      dryRun ? plannedWrite(file, item.source, item.task.task_id) : writeFileChecked(file, item.source, { force, task_id: item.task.task_id }),
    );
  }

  return {
    ok: true,
    dry_run: dryRun,
    wave: outputWave,
    context_pack_path: contextPackRel,
    source_integration_plan: compiled.integration_plan,
    group_count: compiled.dispatchable_groups.length,
    task_count: tasks.length,
    tasks: tasks.map((task) => ({
      task_id: task.task_id,
      shard: task.shard,
      dispatch_group: task.dispatch_group,
      write_scope: task.write_scope,
      must_not_touch: task.must_not_touch,
      acceptance: task.acceptance,
      model_profile: task.model_profile,
      timeout_secs: task.timeout_secs,
    })),
    runtime_projection: {
      config_source: runtimeConfig.source,
      model_profile: effectiveModelProfile,
      timeout_secs: effectiveTimeoutSecs,
    },
    outputs,
    next_commands: [
      `node scripts/prepare-task-runner-wave.mjs --manifest .missiond/tasks/${outputWave}/manifest.lisp`,
      `node scripts/task-runner-dispatch.mjs --manifest .missiond/tasks/${outputWave}/manifest.lisp --max-parallel 4 --json`,
    ],
  };
}

function buildTasks({
  compiled,
  wave,
  taskPrefix,
  contextPackRel,
  estimatedMinutes,
  heartbeatMinutes,
  timeoutSecs,
  modelProfile,
}) {
  const tasks = [];
  const used = new Set();
  let index = 1;
  for (const group of compiled.dispatchable_groups) {
    for (const shard of group.shards) {
      const slug = uniqueTaskSlug(slugify(shard.shard), used);
      const taskId = `${taskPrefix}-${String(index).padStart(2, '0')}-${slug}`;
      index += 1;
      tasks.push({
        task_id: taskId,
        shard: shard.shard,
        owner: shard.owner || 'claudecode',
        summary: shard.summary || `Implement accepted context-pack shard ${shard.shard}.`,
        dispatch_group: group.id,
        context_pack_path: contextPackRel,
        write_scope: shard.write_scope,
        must_not_touch: shard.must_not_touch,
        acceptance: shard.acceptance,
        estimated_minutes: estimatedMinutes,
        heartbeat_minutes: heartbeatMinutes,
        timeout_secs: timeoutSecs,
        model_profile: modelProfile,
        commit_message: `feat(v3): implement ${shard.shard}`,
        wave,
      });
    }
  }
  return tasks;
}

function renderManifest({
  wave,
  contextPackRel,
  nowIso,
  modelProfile,
  estimatedMinutes,
  heartbeatMinutes,
  timeoutSecs,
  tasks,
}) {
  const nodes = tasks
    .map(
      (task) => `  (node :task_id ${task.task_id}
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group ${task.dispatch_group}
        :estimated_minutes ${estimatedMinutes}
        :heartbeat_minutes ${heartbeatMinutes}
        :owner ${quote(task.owner)}
        :model_profile ${modelProfile}
        :timeout_secs ${timeoutSecs}
        :context_pack_path ${quote(contextPackRel)}
        :write_scope ${stringVector(task.write_scope)})`,
    )
    .join('\n');
  return `(task-runner-manifest ${wave}-context-pack-implementation
  :schema "${MANIFEST_SCHEMA}"
  :wave ${wave}
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/${wave}-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  :description "Generated from ${contextPackRel} latest integration-plan."
  :generated_at "${nowIso}"
  :generator "${SCRIPT}"
${nodes})
`;
}

function renderTaskContract(task) {
  return `(task ${task.task_id}
  :schema "${TASK_SCHEMA}"
  :title ${quote(`Implement context-pack shard ${task.shard}`)}
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group ${quote(task.dispatch_group)}
  :estimated-minutes ${task.estimated_minutes}
  :heartbeat-minutes ${task.heartbeat_minutes}
  :context-pack-path ${quote(task.context_pack_path)}
  :goal ${quote(`Implement accepted context-pack shard ${task.shard}. ${task.summary}`)}
  :write-scope ${stringVector(task.write_scope)}
  :must-not-touch ${stringVector(task.must_not_touch)}
  :requirements ["Read the context-pack integration-plan and implement only this accepted shard."
                 "Treat accepted-shards and mapped dispatch-groups as authority."
                 "Do not reinterpret investigator observations as permission to write outside scope."]
  :acceptance ${stringVector(task.acceptance)}
  :commit (:required true :message ${quote(task.commit_message)} :scope-check write-scope-only)
  :report ["Commit hash."
           "Files changed."
           "Acceptance command results."
           "Any blockers or parent-patch needs."])
`;
}

function validateManifestSource(source) {
  const manifests = parseLisp(source, '<generated-manifest>')
    .map((form) => projectManifest(form))
    .filter(Boolean);
  if (manifests.length !== 1) {
    throw new Error(`generated manifest should contain exactly one manifest, got ${manifests.length}`);
  }
  const errors = validateManifestObject(manifests[0]);
  if (errors.length > 0) {
    throw new Error(`generated manifest failed validation:\n${errors.map((e) => `  - ${e}`).join('\n')}`);
  }
}

function validateTaskSources(taskSources) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-context-pack-materialize-'));
  try {
    const files = [];
    for (const { task, source } of taskSources) {
      const file = path.join(tmp, `${task.task_id}.lisp`);
      fs.writeFileSync(file, source);
      files.push(file);
    }
    const proc = spawnSync(process.execPath, [path.resolve('scripts/check-task-contract.mjs'), ...files], {
      cwd: process.cwd(),
      encoding: 'utf8',
      timeout: 30_000,
    });
    if (proc.status !== 0 || proc.error) {
      throw new Error(proc.error?.message ?? proc.stderr ?? proc.stdout);
    }
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function plannedWrite(file, source, taskId = null) {
  return {
    path: path.relative(process.cwd(), file),
    task_id: taskId,
    action: 'planned',
    bytes: Buffer.byteLength(source),
  };
}

function writeFileChecked(file, source, { force = false, task_id = null } = {}) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  if (fs.existsSync(file)) {
    const before = fs.readFileSync(file, 'utf8');
    if (before === source) {
      return { path: path.relative(process.cwd(), file), task_id, action: 'skipped-identical', bytes: Buffer.byteLength(source) };
    }
    if (!force) {
      throw new Error(`refusing to overwrite changed file without --force: ${path.relative(process.cwd(), file)}`);
    }
    fs.writeFileSync(file, source);
    return { path: path.relative(process.cwd(), file), task_id, action: 'overwritten', bytes: Buffer.byteLength(source) };
  }
  fs.writeFileSync(file, source);
  return { path: path.relative(process.cwd(), file), task_id, action: 'written', bytes: Buffer.byteLength(source) };
}

function repoRelative(repo, abs, label) {
  const rel = path.relative(repo, abs);
  if (rel === '' || rel.startsWith('..') || path.isAbsolute(rel)) {
    throw new Error(`${label} must be inside repo root: ${abs}`);
  }
  return normalizeRepoPath(rel);
}

function normalizeRepoPath(value) {
  return value.split(path.sep).join('/');
}

function stringVector(values) {
  return `[${values.map((value) => quote(value)).join(' ')}]`;
}

function quote(value) {
  return JSON.stringify(String(value));
}

function slugify(value) {
  const slug = String(value)
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, '-')
    .replace(/^[^a-z0-9]+/, '')
    .replace(/[^a-z0-9]+$/, '');
  return slug || 'shard';
}

function uniqueTaskSlug(base, used) {
  let candidate = base;
  let i = 2;
  while (used.has(candidate)) {
    candidate = `${base}-${i}`;
    i += 1;
  }
  used.add(candidate);
  return candidate;
}

function validateToken(value, label) {
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(String(value))) {
    throw new Error(`${label} must be a compact token, got ${JSON.stringify(value)}`);
  }
}

function need(args, index, flag) {
  const value = args[index];
  if (!value || value.startsWith('--')) fail(`${flag} requires a value`);
  return value;
}

function parsePositiveInt(value, flag) {
  if (!/^[1-9][0-9]*$/.test(value)) fail(`${flag} requires a positive integer`);
  return Number.parseInt(value, 10);
}

function isoNow() {
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function fail(message) {
  console.error(`${message}\n\n${usage}`);
  process.exit(2);
}

function runFixtures() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-materialize-wave-'));
  try {
    const packPath = path.join(tmp, '.missiond/tasks/wave99/context-pack.lisp');
    fs.mkdirSync(path.dirname(packPath), { recursive: true });
    fs.writeFileSync(packPath, fixtureContextPack());
    const blueprintPath = path.join(tmp, '.missiond/v3/missiond-blueprint.lisp');
    fs.mkdirSync(path.dirname(blueprintPath), { recursive: true });
    fs.writeFileSync(blueprintPath, fixtureBlueprint());
    const result = materializeContextPackWave({
      contextPackPath: packPath,
      repoRoot: tmp,
      nowIso: '2026-04-29T00:00:00Z',
    });
    assert(result.task_count === 2, 'expected two generated tasks');
    assert(result.outputs.manifest.action === 'written', 'manifest should be written');
    assert(result.outputs.contracts.every((c) => c.action === 'written'), 'contracts should be written');
    assert(result.runtime_projection.model_profile === 'fixture-opus-4-7', 'expected model profile from V3 workstation-config');
    assert(result.runtime_projection.timeout_secs === 3660, 'expected timeout from V3 workstation-config');
    assert(result.tasks.every((task) => task.model_profile === 'fixture-opus-4-7'), 'tasks should carry projected model profile');
    assert(result.tasks.every((task) => task.timeout_secs === 3660), 'tasks should carry projected timeout');

    const manifest = path.join(tmp, '.missiond/tasks/wave99/manifest.lisp');
    assert(fs.existsSync(manifest), 'manifest should exist');
    const manifestSource = fs.readFileSync(manifest, 'utf8');
    assert(manifestSource.includes(':model_profile fixture-opus-4-7'), 'manifest should project V3 model profile');
    assert(manifestSource.includes(':timeout_secs 3660'), 'manifest should project V3 timeout');
    const manifestCheck = spawnSync(process.execPath, [path.resolve('scripts/check-task-runner-manifest.mjs'), manifest], {
      cwd: process.cwd(),
      encoding: 'utf8',
    });
    assert(manifestCheck.status === 0, manifestCheck.stderr || manifestCheck.stdout);
    const contractCheck = spawnSync(
      process.execPath,
      [
        path.resolve('scripts/check-task-contract.mjs'),
        path.join(tmp, '.missiond/tasks/wave99/wave99-01-alpha.lisp'),
        path.join(tmp, '.missiond/tasks/wave99/wave99-02-beta.lisp'),
      ],
      { cwd: process.cwd(), encoding: 'utf8' },
    );
    assert(contractCheck.status === 0, contractCheck.stderr || contractCheck.stdout);

    const dry = materializeContextPackWave({
      contextPackPath: packPath,
      repoRoot: tmp,
      dryRun: true,
      taskPrefix: 'wave100',
      wave: 'wave100',
      nowIso: '2026-04-29T00:01:00Z',
    });
    assert(dry.outputs.manifest.action === 'planned', 'dry run should not write');
    assert(!fs.existsSync(path.join(tmp, '.missiond/tasks/wave100/manifest.lisp')), 'dry run should not materialize files');

    fs.writeFileSync(
      packPath,
      fixtureContextPack({
        dispatchGroups: '[A]',
        integrationId: 'i4',
      }),
    );
    assertThrows(
      () =>
        materializeContextPackWave({
          contextPackPath: packPath,
          repoRoot: tmp,
          wave: 'wave101',
          taskPrefix: 'wave101',
          nowIso: '2026-04-29T00:02:00Z',
        }),
      'names-only dispatch groups should be rejected',
    );

    return { ok: true, cases: 3 };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function fixtureContextPack({ dispatchGroups = '[(group :id A :shards [alpha]) (group :id B :shards [beta])]', integrationId = 'i3' } = {}) {
  return `(context-pack wave99-context-pack
  :schema "missiond.context-pack.v1"
  :wave wave99
  :purpose "Materialize dispatchable code-worker shards."
  :write-model append-only
  :sequence 3
  (shard-proposal :id s1 :agent context-a :seq 1 :at "2026-04-29T00:00:00Z" :shard alpha :owner worker-a :summary "Alpha summary." :write-scope ["scripts/alpha.mjs"] :must-not-touch ["packages/**"] :acceptance ["node scripts/check-v3-code-isomorphism-complete.mjs"])
  (shard-proposal :id s2 :agent context-b :seq 2 :at "2026-04-29T00:00:01Z" :shard beta :owner worker-b :summary "Beta summary." :write-scope ["scripts/beta.mjs"] :must-not-touch ["scripts/alpha.mjs"] :acceptance ["git diff --check"])
  (integration-plan :id ${integrationId} :agent integrator :seq 3 :at "2026-04-29T00:00:02Z" :summary "accept both" :accepted-shards [alpha beta] :dispatch-groups ${dispatchGroups}))`;
}

function fixtureBlueprint() {
  return `(missiond-blueprint
  (workstation-config
    (slot-template coder
      :role coder
      :default-model-profile fixture-opus-4-7)
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

function assertThrows(fn, message) {
  let threw = false;
  try {
    fn();
  } catch {
    threw = true;
  }
  if (!threw) throw new Error(message);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
