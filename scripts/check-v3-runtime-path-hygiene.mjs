#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-runtime-path-hygiene.mjs [--json] [--dry-fixture]

Checks that public/runtime-facing documentation names V3 runtime artifact paths
first, while V2 paths appear only as explicit legacy fallbacks or historical
design sources. It also pins the SSOT retrieval scope that keeps generated
runtime artifacts out of default blueprint review/search.
`;

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  gitignore: '.gitignore',
  coreExecution: 'crates/missiond-core/src/event/events/execution.rs',
  coreObservability: 'crates/missiond-core/src/event/events/observability.rs',
  mcpExecution: 'crates/missiond-mcp/src/tools/knowledge/agent_execution.rs',
  mcpPlan: 'crates/missiond-mcp/src/tools/knowledge/plan.rs',
  mcpWorkflow: 'crates/missiond-mcp/src/tools/knowledge/workflow.rs',
  deployScript: 'scripts/deploy-daemon.sh',
  daemonCompiledSnapshot: 'crates/missiond-daemon/src/context/v3_blueprint_runtime/compiled_snapshot.rs',
  jarvisMonitor: 'crates/missiond-core/src/ws/server.rs',
  contextGather: 'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
  masterControl: 'crates/missiond-daemon/src/engine/master_control.rs',
  autopilotOrgan: 'crates/missiond-daemon/src/organism/autopilot_organ.rs',
  bootContext: '.missiond/v3/evidence/codex-boot-context.lisp',
  requestRuntime: '.missiond/v3/shards/request-runtime.lisp',
  controlPlaneRuntime: '.missiond/v3/shards/control-plane-runtime.lisp',
  genomeRuntime: '.missiond/v3/shards/implementation/runtime-surfaces.lisp',
};

const REQUIRED = {
  blueprint: [
    'ssot-retrieval-scope',
    'active-authoring',
    'warm-evidence',
    'cold-runtime',
    '.missiond/v3/runtime/**',
    '.missiond/v3/runtime/lisp-code-sync/*.report.lisp',
    '.missiond/v3/runtime/jarvis-smoke/*.json',
    'runtime_artifacts',
    'MISSIOND_RUNTIME_DIR',
    '$HOME/.missiond/runtime/<repo-id>',
    'include_runtime=true',
    '.missiond/v3/runtime/capability-usage-review.json',
    'node scripts/check-v3-runtime-path-hygiene.mjs',
    'New runtime work should cite v3 first',
  ],
  gitignore: [
    '.missiond/v3/runtime/**',
    '.missiond/v3/runtime/executions/*.lisp',
    '.missiond/v3/runtime/plans/*.evidence.json',
    '.missiond/v3/runtime/capability-usage-review.json',
    '.missiond/v3/runtime/nightly-evolution/*.report.lisp',
    '.missiond/v3/runtime/lisp-code-sync/*.report.lisp',
    '.missiond/v3/runtime/jarvis-smoke/*.json',
    '.missiond/v3/runtime/jarvis-smoke/*.lisp',
  ],
  coreExecution: [
    '.missiond/v3/runtime/executions/<id>.lisp',
    '.missiond/v3/runtime/plans/<plan_id>.evidence.json',
  ],
  coreObservability: ['.missiond/v3/runtime/capability-usage-review.json'],
  mcpExecution: ['.missiond/v3/runtime/executions/<id>.lisp'],
  mcpPlan: ['.missiond/v3/runtime/plans/<plan_id>.evidence.json'],
  mcpWorkflow: ['.missiond/v3/runtime/plans/<plan_id>.evidence.json'],
  deployScript: [
    'MISSIOND_RUNTIME_DIR',
    'MISSIOND_COMPILED_RUNTIME_DIR',
    'MISSIOND_CLEAN_REPO_RUNTIME_CACHE',
    'cleanup_repo_runtime_cache',
    'master-control-checkpoint.lisp',
    '$repo_runtime/context-gather',
    '$repo_runtime/genome',
    'node scripts/compile-v3-runtime.mjs --json --out-dir "$COMPILED_RUNTIME_DIR"',
  ],
  daemonCompiledSnapshot: [
    'MISSIOND_COMPILED_RUNTIME_DIR',
    'MISSIOND_RUNTIME_DIR',
  ],
  jarvisMonitor: [
    'MISSIOND_COMPILED_RUNTIME_DIR',
    'MISSIOND_RUNTIME_DIR',
  ],
  contextGather: [
    'runtime_environment',
    'MISSIOND_RUNTIME_DIR',
    'MISSIOND_COMPILED_RUNTIME_DIR',
    'context-capsules',
    'monitor_endpoints',
    'canonical_local_http',
    'http://127.0.0.1:9120/api/monitor/jarvis',
    'canonical_public_https',
    'https://auth.xiaojinpro.com/jarvis/api/monitor/jarvis',
    'Repo .missiond/v3/runtime/** is dev/cold evidence only',
    'context-gather-worker',
    'context_gather_runtime_dir',
    'context_gather_worker_visible_dir',
    'context_capsule_runtime_dir',
  ],
  masterControl: [
    'MISSIOND_RUNTIME_DIR',
    'missiond_runtime_dir_from_env',
    'CHECKPOINT_RUNTIME_PATH',
    'MASTER_CONTEXT_PACK_RUNTIME_DIR',
  ],
  autopilotOrgan: [
    'MISSIOND_RUNTIME_DIR',
    'missiond_runtime_dir',
    'autopilot_runtime_path',
    'SHADOW_RELATIVE_PATH',
    'ACTIVATION_RELATIVE_PATH',
  ],
  bootContext: [
    'runtime artifacts live under MISSIOND_RUNTIME_DIR and MISSIOND_COMPILED_RUNTIME_DIR',
    'repo .missiond/v3/runtime/** is dev/cold evidence only',
    'runtime_environment',
    'canonical Jarvis monitor endpoints',
  ],
  requestRuntime: [
    'runtime-environment',
    'runtime_environment',
    'MISSIOND_RUNTIME_DIR/context-gather',
    'canonical Jarvis monitor endpoints',
    'runtime_environment.monitor_endpoints.canonical_local_http',
    'canonical_public_https',
    'repo .missiond/v3/runtime/** is dev/cold evidence only',
  ],
  controlPlaneRuntime: [
    '$MISSIOND_RUNTIME_DIR/master-control-checkpoint.lisp',
    'repo .missiond/v3/runtime kept only as dev fallback',
  ],
  genomeRuntime: [
    'MISSIOND_RUNTIME_DIR/genome',
    'repo .missiond/v3/runtime/genome is dev/cold evidence only',
  ],
};

const FORBIDDEN = {
  coreExecution: [
    'Durable evidence remains the on-disk `<project_root>/.missiond/v2/<id>.lisp`',
    'lives in `<project_root>/.missiond/v2/plans/<plan_id>.evidence.json`',
  ],
  coreObservability: ['JSON sidecar at `.missiond/v2/capability-usage-review.json`'],
  mcpExecution: ['13 actions over .missiond/v2/<id>.lisp'],
  mcpPlan: ['record_evidence 写 sidecar `<project>/.missiond/v2/plans/<plan_id>.evidence.json`'],
  mcpWorkflow: [
    'distill uses it to locate `.missiond/v2/plans/<plan_id>.evidence.json`',
    'appends ONE chain-record evidence row to `<project_root>/.missiond/v2/plans/<plan_id>.evidence.json`',
    'evidence row 到 `<project_root>/.missiond/v2/plans/<plan_id>.evidence.json`',
  ],
  autopilotOrgan: [
    'const SHADOW_PATH: &str = ".missiond/v3/runtime/genome/autopilot-shadow.json"',
    'const ACTIVATION_PATH: &str = ".missiond/v3/runtime/genome/activation.json"',
  ],
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

  const repoRoot = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(repoRoot);
  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 runtime path hygiene check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 runtime path hygiene check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }
  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(FILES)) {
    const abs = path.join(root, rel);
    try {
      sources[key] =
        key === 'blueprint'
          ? readBlueprintWithEvidenceSidecars(root, rel)
          : fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  for (const [key, needles] of Object.entries(REQUIRED)) {
    for (const needle of needles) {
      if (!sources[key].includes(needle)) {
        diagnostics.push({
          file: FILES[key],
          message: `missing V3 runtime path contract: ${needle}`,
        });
      }
    }
  }

  for (const [key, needles] of Object.entries(FORBIDDEN)) {
    for (const needle of needles) {
      if (sources[key].includes(needle)) {
        diagnostics.push({
          file: FILES[key],
          message: `stale V2 runtime path still presented as canonical: ${needle}`,
        });
      }
    }
  }

  return diagnostics;
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-runtime-paths-'));
  for (const rel of Object.values(FILES)) {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  }
  write(root, 'blueprint', `
(missiond-blueprint
  (ssot-retrieval-scope
    (tier active-authoring :paths [".missiond/v3/missiond-blueprint.lisp"])
    (tier warm-evidence :paths [".missiond/v3/evidence/*.lisp"])
    (tier cold-runtime
      :paths [".missiond/v3/runtime/**"]
      :examples [".missiond/v3/runtime/lisp-code-sync/*.report.lisp"
                 ".missiond/v3/runtime/jarvis-smoke/*.json"]
      :catalog runtime_artifacts
      :runtime-root "MISSIOND_RUNTIME_DIR"
      :default-runtime-root "$HOME/.missiond/runtime/<repo-id>"
      :rule "Cold runtime artifacts are excluded unless include_runtime=true is explicit."))
  (capability-governance-policy
    :review-sidecar ".missiond/v3/runtime/capability-usage-review.json")
  (compression-contract
    :rule "New runtime work should cite v3 first, then v2 source-index for historical evidence."
    :checks ["node scripts/check-v3-runtime-path-hygiene.mjs"]))`);
  write(root, 'gitignore', `
.missiond/v3/runtime/executions/*.lisp
.missiond/v3/runtime/**
.missiond/v3/runtime/plans/*.evidence.json
.missiond/v3/runtime/capability-usage-review.json
.missiond/v3/runtime/nightly-evolution/*.report.lisp
.missiond/v3/runtime/lisp-code-sync/*.report.lisp
.missiond/v3/runtime/jarvis-smoke/*.json
.missiond/v3/runtime/jarvis-smoke/*.lisp`);
  write(root, 'coreExecution', `
//! Durable evidence remains the on-disk <project_root>/.missiond/v3/runtime/executions/<id>.lisp companion file.
/// lives in <project_root>/.missiond/v3/runtime/plans/<plan_id>.evidence.json`);
  write(root, 'coreObservability', `
/// JSON sidecar at .missiond/v3/runtime/capability-usage-review.json`);
  write(root, 'mcpExecution', `
13 actions over .missiond/v3/runtime/executions/<id>.lisp companion logs, with legacy .missiond/v2/<id>.lisp fallback`);
  write(root, 'mcpPlan', `
record_evidence writes canonical <project>/.missiond/v3/runtime/plans/<plan_id>.evidence.json`);
  write(root, 'mcpWorkflow', `
distill uses .missiond/v3/runtime/plans/<plan_id>.evidence.json; legacy .missiond/v2/plans/<plan_id>.evidence.json fallback`);
  write(root, 'jarvisMonitor', `
MISSIOND_COMPILED_RUNTIME_DIR
MISSIOND_RUNTIME_DIR`);
  write(root, 'daemonCompiledSnapshot', `
MISSIOND_COMPILED_RUNTIME_DIR
MISSIOND_RUNTIME_DIR`);
  write(root, 'deployScript', `
MISSIOND_RUNTIME_DIR
MISSIOND_COMPILED_RUNTIME_DIR
MISSIOND_CLEAN_REPO_RUNTIME_CACHE
cleanup_repo_runtime_cache
master-control-checkpoint.lisp
$repo_runtime/context-gather
$repo_runtime/genome
node scripts/compile-v3-runtime.mjs --json --out-dir "$COMPILED_RUNTIME_DIR"`);
  write(root, 'contextGather', `
runtime_environment
MISSIOND_RUNTIME_DIR
MISSIOND_COMPILED_RUNTIME_DIR
context-capsules
Repo .missiond/v3/runtime/** is dev/cold evidence only
context-gather-worker
context_gather_runtime_dir
context_gather_worker_visible_dir
context_capsule_runtime_dir`);
  write(root, 'masterControl', `
MISSIOND_RUNTIME_DIR
missiond_runtime_dir_from_env
CHECKPOINT_RUNTIME_PATH
MASTER_CONTEXT_PACK_RUNTIME_DIR`);
  write(root, 'autopilotOrgan', `
MISSIOND_RUNTIME_DIR
missiond_runtime_dir
autopilot_runtime_path
SHADOW_RELATIVE_PATH
ACTIVATION_RELATIVE_PATH`);
  write(root, 'bootContext', `
runtime artifacts live under MISSIOND_RUNTIME_DIR and MISSIOND_COMPILED_RUNTIME_DIR
repo .missiond/v3/runtime/** is dev/cold evidence only
runtime_environment`);
  write(root, 'requestRuntime', `
runtime-environment
runtime_environment
MISSIOND_RUNTIME_DIR/context-gather
repo .missiond/v3/runtime/** is dev/cold evidence only`);
  write(root, 'controlPlaneRuntime', `
$MISSIOND_RUNTIME_DIR/master-control-checkpoint.lisp
repo .missiond/v3/runtime kept only as dev fallback`);
  write(root, 'genomeRuntime', `
MISSIOND_RUNTIME_DIR/genome
repo .missiond/v3/runtime/genome is dev/cold evidence only`);
  return root;
}

function write(root, key, source) {
  fs.writeFileSync(path.join(root, FILES[key]), source.trimStart());
}

main();
