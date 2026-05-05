#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';
import { spawnSync } from 'node:child_process';

const usage = `Usage:
  node scripts/check-v3-context-pack-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 context-pack Lisp/code isomorphism contract:
  - blueprint declares multi-agent append-only context-pack architecture.
  - implementation-map exposes context-pack as a code-aligned surface.
  - scripts/check-context-pack.mjs owns structural validation for context-pack v1.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  checker: 'scripts/check-context-pack.mjs',
  appender: 'scripts/context-pack-append.mjs',
  compiler: 'scripts/context-pack-compile-shards.mjs',
  materializer: 'scripts/context-pack-materialize-wave.mjs',
  runner: 'scripts/context-pack-run-wave.mjs',
  v3Runtime: 'scripts/lib/v3_workstation_runtime.mjs',
  taskDelegate: 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '-h' || arg === '--help') {
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

  const repoRoot = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  if (diagnostics.length === 0) {
    for (const script of [
      DEFAULT_FILES.checker,
      DEFAULT_FILES.appender,
      DEFAULT_FILES.compiler,
      DEFAULT_FILES.materializer,
      DEFAULT_FILES.runner,
    ]) {
      const scriptPath =
        dryFixture && script === DEFAULT_FILES.runner
          ? path.resolve(process.cwd(), script)
          : path.join(repoRoot, script);
      const scriptCwd = dryFixture && script === DEFAULT_FILES.runner ? process.cwd() : repoRoot;
      const proc = spawnSync(process.execPath, [scriptPath, '--dry-fixture'], {
        cwd: scriptCwd,
        encoding: 'utf8',
        timeout: 30_000,
      });
      if (proc.status !== 0 || proc.error) {
        diagnostics.push({
          file: script,
          message: `dry fixture failed: ${proc.error?.message ?? proc.stderr ?? proc.stdout}`,
        });
      }
    }
  }

  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 context-pack Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(
      `v3 context-pack Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }
  process.exit(result.ok ? 0 : 1);
}

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

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'context-pack',
    'missiond.context-pack.v1',
    'multi-agent append-only',
    'shard-proposal',
    'integration-plan',
    'accepted-shards',
    'dispatch-groups',
    'context-pack-compile-shards',
    'context-pack-materialize-wave',
    'context-pack-run-wave',
    'two-stage',
    'code workers consume',
    '(v2-item context-pack-two-stage-parallel-work',
    ':status runtime-projected',
    'materialize-wave',
    'run-wave',
    'task-runner-manifest',
    'task-contracts',
    'dispatch-policy context-pack-run-wave',
    ':default_max_parallel 4',
    'default max_parallel from workstation-config dispatch-policy context-pack-run-wave',
    'create-only initializes missing shared-memory/session-trace ledgers before prepare',
    '(surface context-pack',
    ':status "code-aligned"',
    'scripts/check-context-pack.mjs',
    'scripts/context-pack-append.mjs',
    'scripts/context-pack-compile-shards.mjs',
    'scripts/context-pack-materialize-wave.mjs',
    'scripts/context-pack-run-wave.mjs',
    'scripts/lib/v3_workstation_runtime.mjs',
    'node scripts/check-v3-context-pack-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.checker, sources.checker, [
    'export const SCHEMA = \'missiond.context-pack.v1\'',
    'export const CONTEXT_PACK_HEAD = \'context-pack\'',
    'export const ENTRY_HEADS',
    '\'shard-proposal\'',
    '\'integration-plan\'',
    'validateContextPackSource',
    'validateContextPackFiles',
    'append-only',
    'accepted shards',
    'write-scope',
    'must-not-touch',
    '(group :id A :shards [alpha beta])',
    'runFixtures',
  ]);

  requireAll(diagnostics, files.appender, sources.appender, [
    'appendContextPackEntry',
    'withLock',
    'nextSequence',
    'validateContextPackSource',
    'spliceBeforeFinalParen',
    'context-pack-append OK',
    '--pack <context-pack.lisp>',
    '--dispatch-group-shards',
  ]);

  requireAll(diagnostics, files.compiler, sources.compiler, [
    'compileContextPackSource',
    'dispatchable_groups',
    'group_mode',
    'mapped',
    'names_only',
    'context-pack shard compile OK',
  ]);

  requireAll(diagnostics, files.materializer, sources.materializer, [
    'materializeContextPackWave',
    'compileContextPackSource',
    'context-pack remains the SSOT',
    'task-runner-manifest.v2',
    'task-contract.v1',
    'mapped dispatch groups',
    'loadWorkstationRuntimeConfigForRepo',
    'defaultModelProfileForTemplate(\'coder\')',
    'clampTimeoutSecs',
    'model_profile',
    'timeout_secs',
    'runtime_projection',
    'context_pack_path',
    '--allow-default-config',
    'context-pack-run-wave.mjs',
    'prepare-task-runner-wave.mjs',
    'task-runner-dispatch.mjs',
  ]);

  requireAll(diagnostics, files.runner, sources.runner, [
    'missiond.context-pack-run-wave.v0',
    'runContextPackWave',
    'materializeContextPackWave',
    'prepareWave',
    'runDispatch',
    'submitDispatch',
    'loadWorkstationRuntimeConfigForRepo',
    'contextPackMaxParallel',
    'ensureWaveLedgers',
    'writeCreateOnly',
    '--skip-ledger-init',
    '--context-pack <context-pack.lisp>',
    '--max-parallel <n|all>',
    '--apply',
    'context-pack remains the SSOT',
    'dispatch descriptor',
    'never calls the daemon or spawns workers unless --apply',
    'modelProfile',
    'timeoutSecs',
    'max_parallel',
    'ledger_init',
    'skipped-existing',
    'allowDefaultConfig',
    'BoardTask ID: assigned by mission_task_delegate',
  ]);

  requireAll(diagnostics, files.v3Runtime, sources.v3Runtime, [
    'export class WorkstationRuntimeConfig',
    'export function loadWorkstationRuntimeConfigForRepo',
    'export function parseWorkstationRuntimeConfig',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'workstation-config',
    'slot-template',
    'timeout-policy',
    'boardtask-dispatch',
    'defaultModelProfileForTemplate',
    'clampTimeoutSecs',
    'DEFAULT_MODEL_PROFILE',
    'DEFAULT_TIMEOUT_SECS',
    'DEFAULT_CONTEXT_PACK_MAX_PARALLEL',
    'MIN_CONTEXT_PACK_MAX_PARALLEL',
    'MAX_CONTEXT_PACK_MAX_PARALLEL',
    'contextPackMaxParallel',
    'dispatch-policy',
    'context-pack-run-wave',
    ':default_max_parallel',
  ]);

  requireAll(diagnostics, files.taskDelegate, sources.taskDelegate, [
    'materialize_swarm_context_pack',
    'render_swarm_context_pack',
    'missiond.swarm-context-pack.v1',
    ':target_projects',
    'SwarmTargetProject',
    'target_project_ids',
    'targetProjectIds',
    'resolve_swarm_target_projects',
    'context_pack_materialized',
    'SWARM_CONTEXT_PACK_COUNTER',
    'timestamp_subsec_nanos',
    'fetch_add',
    'auto_provision_slots',
    '"provisioned_slots": provisioned_slots',
    'tokio::fs::create_dir_all',
    'tokio::fs::write',
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

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-context-pack-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (v2-convergence-map
    (v2-item context-pack-two-stage-parallel-work
      :status runtime-projected))
  (artifact-contracts
    (artifact context-pack
      :schema "missiond.context-pack.v1"
      :writer context-pack-append
      :required [:id :agent :seq]))
  (multi-agent-context-pack
    :write-model "multi-agent append-only"
    :entries [claim observation anchor shard-proposal conflict integration-plan]
    :merge "two-stage: investigators append proposals; code workers consume integration-plan accepted-shards and dispatch-groups via context-pack-compile-shards then context-pack-materialize-wave"
    :flow [compile-shards materialize-wave run-wave]
    :egress [task-runner-manifest task-contracts])
  (workstation-config
    (dispatch-policy context-pack-run-wave
      :default_max_parallel 4
      :min_parallel 1
      :max_parallel 8))
  (implementation-map
    (surface context-pack
      :status "code-aligned"
	      :code ["scripts/check-context-pack.mjs"
	             "scripts/context-pack-append.mjs"
	             "scripts/context-pack-compile-shards.mjs"
	             "scripts/context-pack-materialize-wave.mjs"
	             "scripts/context-pack-run-wave.mjs"
	             "scripts/lib/v3_workstation_runtime.mjs"
	             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"]
		      :note "context-pack-materialize-wave context-pack-run-wave materialize-wave run-wave task-runner-manifest task-contracts default max_parallel from workstation-config dispatch-policy context-pack-run-wave create-only initializes missing shared-memory/session-trace ledgers before prepare fixture mission_swarm_run MUST materialize missiond.swarm-context-pack.v1"))
  (compression-contract
    :checks ["node scripts/check-v3-context-pack-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.checker, fs.readFileSync(DEFAULT_FILES.checker, 'utf8'));
  writeFixture(root, DEFAULT_FILES.appender, fs.readFileSync(DEFAULT_FILES.appender, 'utf8'));
  writeFixture(root, DEFAULT_FILES.compiler, fs.readFileSync(DEFAULT_FILES.compiler, 'utf8'));
  writeFixture(root, DEFAULT_FILES.materializer, fs.readFileSync(DEFAULT_FILES.materializer, 'utf8'));
  writeFixture(root, DEFAULT_FILES.runner, fs.readFileSync(DEFAULT_FILES.runner, 'utf8'));
  writeFixture(root, DEFAULT_FILES.v3Runtime, fs.readFileSync(DEFAULT_FILES.v3Runtime, 'utf8'));
  writeFixture(root, DEFAULT_FILES.taskDelegate, fs.readFileSync(DEFAULT_FILES.taskDelegate, 'utf8'));
  writeFixture(root, 'scripts/lib/missiond_lisp.mjs', fs.readFileSync('scripts/lib/missiond_lisp.mjs', 'utf8'));
  writeFixture(root, 'scripts/check-task-contract.mjs', fs.readFileSync('scripts/check-task-contract.mjs', 'utf8'));
  writeFixture(root, 'scripts/check-task-runner-manifest.mjs', fs.readFileSync('scripts/check-task-runner-manifest.mjs', 'utf8'));
  return root;
}

function writeFixture(root, rel, text) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text.trimStart());
}

main();
