#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-grounded-dispatch-isomorphism.mjs [--json] [--repo <path>]

Checks the V3 grounded-dispatch contract:
  - SSOT declares grounded-dispatch-policy, context-gather-artifact,
    task-delegate-grounding-gate, and autopilot-grounding-gate.
  - mission_context_gather can persist a durable context-gather artifact and
    return grounding_context_id/context_pack_path.
  - mission_task_delegate and mission_swarm_run enforce grounding before broad
    dispatch and fail fast when gather cannot produce a grounded artifact.
  - Autopilot refuses ungrounded broad BoardTasks before PTY dispatch.
`;

let json = false;
let repoRoot = process.cwd();
const args = process.argv.slice(2);
for (let i = 0; i < args.length; i += 1) {
  const arg = args[i];
  if (arg === '--json') {
    json = true;
  } else if (arg === '--repo') {
    repoRoot = path.resolve(args[++i] ?? '');
  } else if (arg === '--help' || arg === '-h') {
    console.log(usage);
    process.exit(0);
  } else {
    console.error(`unknown arg: ${arg}`);
    console.error(usage);
    process.exit(2);
  }
}

const checks = [
  [
    'ssot grounded-dispatch policy',
    '.missiond/v3/shards/request-runtime.lisp',
    [
      '(grounded-dispatch-policy',
      ':schema "missiond.grounded-dispatch-policy.v1"',
      '(function context-gather-artifact',
      '(function task-delegate-grounding-gate',
      '(function autopilot-grounding-gate',
      'grounding_context_id',
      'mission_context_gather(persist=true)',
      'GROUNDING_REQUIRED',
      'node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json',
    ],
  ],
  [
    'intent workflow runtime gate',
    '.missiond/workflows/intent-intake-grounding.lisp',
    [
      'mission_context_gather',
      'persisted grounding_context_id',
      'exact_shard_ready',
      'accepted_shard_id, context_pack_path, and write_scope',
    ],
  ],
  [
    'work-order lifecycle grounding gate',
    '.missiond/workflows/work-order-lifecycle.lisp',
    [
      '(gate grounding-required-before-worker',
      'mission_context_gather(persist=true)',
      'grounding_context_id',
      'runtime block',
    ],
  ],
  [
    'context gather persistent artifact',
    'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
    [
      'persist: Option<bool>',
      'task_id: Option<String>',
      'source_id: Option<String>',
      '"missiond.context-gather-artifact.v1"',
      'put_json_artifact',
      '"context-gather"',
      '"grounding_context_id"',
      '"context_pack_path"',
      '"shared-artifact://',
      '"sources_used"',
    ],
  ],
  [
    'shared memory JSON artifact helper',
    'crates/missiond-daemon/src/engine/shared_memory.rs',
    [
      'pub(crate) async fn put_json_artifact',
      'put_artifact_bytes',
      'serde_json::to_vec',
      '"application/json"',
    ],
  ],
  [
    'task delegate grounding gate',
    'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
    [
      'dispatch_grounding_required',
      'gather_dispatch_grounding',
      'apply_grounding_to_metadata',
      'context_gather::handle',
      '"mission_context_gather"',
      '"persist": true',
      '"GROUNDING_REQUIRED"',
      'grounding_context_id',
      'grounding_sources',
      'context_pack_path',
    ],
  ],
  [
    'swarm grounding contract',
    'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
    [
      '"SWARM_GROUNDING_CONTEXT_REQUIRED"',
      'grounding_context_id',
      'grounding_artifact',
      ':grounding_context_id',
      '- grounding_context_id:',
      'accepted shards must reference an existing grounding_context_id',
    ],
  ],
  [
    'autopilot grounding gate',
    'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
    [
      'autopilot_grounding_gate_reason',
      'block_task_for_grounding_required',
      'board_task_exact_shard_ready',
      'grounding_context_id',
      'Autopilot refused broad dispatch without grounding_context_id',
      'GROUNDING_REQUIRED',
    ],
  ],
  [
    'aggregate registration',
    'scripts/check-v3-code-isomorphism-complete.mjs',
    ['scripts/check-v3-grounded-dispatch-isomorphism.mjs'],
  ],
];

const diagnostics = [];
for (const [label, rel, needles] of checks) {
  const file = path.join(repoRoot, rel);
  let text = '';
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (error) {
    diagnostics.push({
      label,
      file: rel,
      message: `missing file: ${error.message}`,
    });
    continue;
  }
  for (const needle of needles) {
    if (!text.includes(needle)) {
      diagnostics.push({
        label,
        file: rel,
        needle,
        message: `missing grounded-dispatch anchor: ${needle}`,
      });
    }
  }
}

const result = {
  ok: diagnostics.length === 0,
  checked: checks.length,
  diagnostics,
};

if (json) {
  console.log(JSON.stringify(result, null, 2));
} else if (result.ok) {
  console.log('v3 grounded-dispatch Lisp/code isomorphism check OK');
} else {
  for (const d of diagnostics) {
    console.error(`${d.file}: ${d.message}`);
  }
  console.error(
    `v3 grounded-dispatch Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
  );
}

process.exit(result.ok ? 0 : 1);
