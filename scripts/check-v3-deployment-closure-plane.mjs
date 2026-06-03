#!/usr/bin/env node

import fs from 'node:fs';

const json = process.argv.includes('--json');

const files = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  shard: '.missiond/v3/shards/deployment-closure-plane.lisp',
  index: '.missiond/v3/shards/index.lisp',
  compiler: 'scripts/compile-v3-runtime.mjs',
  projectRegistry: 'crates/missiond-daemon/src/handlers/knowledge/project/registry.rs',
  timeline: 'crates/missiond-daemon/src/handlers/comm/timeline.rs',
  deployDaemon: 'scripts/deploy-daemon.sh',
  eventbridge: 'crates/missiond-daemon/src/bus/v2_subscribers.rs',
  autopilot: 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
  taskDelegate: 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
  serviceLayerTemplate: '.missiond/v3/shards/universe/service-layer-template.lisp',
  productScaffold: 'scripts/scaffold-product-deployment-closure.mjs',
};

const required = [
  ['shard', 'deployment-closure-plane'],
  ['shard', 'ReleaseEvidence'],
  ['shard', 'ClosureVerdict'],
  ['shard', 'ReleaseLease'],
  ['shard', 'runtime_digest_mismatch'],
  ['shard', 'release_lease_conflict'],
  ['shard', 'candidate git_full_sha'],
  ['shard', 'commit-regression override'],
  ['shard', 'compiled runtime projections as release artifacts'],
  ['shard', 'release-evidence-review'],
  ['shard', 'closure-verdict-review'],
  ['blueprint', 'deployment-closure-plane'],
  ['blueprint', 'shards/deployment-closure-plane.lisp'],
  ['index', 'shards/deployment-closure-plane.lisp'],
  ['compiler', 'release_lease_required'],
  ['compiler', 'artifact_lane'],
  ['compiler', 'target_side_build_allowed'],
  ['compiler', 'support_catalog'],
  ['compiler', 'runtime-target'],
  ['compiler', 'runtime_target_missing'],
  ['compiler', 'diagnostic_profiles'],
  ['compiler', 'closure_state_machine'],
  ['projectRegistry', 'production_release_projection'],
  ['projectRegistry', 'ClosureVerdict'],
  ['timeline', 'closure_verdict'],
  ['timeline', 'external_service_event'],
  ['deployDaemon', 'release-evidence.json'],
  ['deployDaemon', 'closure-verdict.json'],
  ['deployDaemon', 'release-lease.json'],
  ['deployDaemon', 'missiond.release-evidence.v1'],
  ['deployDaemon', 'missiond.closure-verdict.v1'],
  ['deployDaemon', 'missiond.release-lease.v1'],
  ['deployDaemon', 'expected_active_commit'],
  ['deployDaemon', 'git_full_sha'],
  ['deployDaemon', 'commit-ancestry-guard'],
  ['deployDaemon', 'release-local compiled runtime dir'],
  ['deployDaemon', 'MISSIOND_COMPILED_RUNTIME_DIR'],
  ['deployDaemon', 'compiled-runtime'],
  ['eventbridge', 'DEPLOY_OPS_OUTPUT_CONTRACT'],
  ['eventbridge', 'allowed_output_artifacts'],
  ['eventbridge', 'mutation_requires_approval'],
  ['autopilot', 'missing-deploy-ops-output-artifact'],
  ['taskDelegate', 'DEPLOY_OPS_APPROVAL_REQUIRED'],
  ['taskDelegate', 'deploy_ops_write_scope_without_approval'],
  ['serviceLayerTemplate', 'deployment-closure-bundle-standard'],
  ['serviceLayerTemplate', 'scripts/scaffold-product-deployment-closure.mjs'],
  ['serviceLayerTemplate', 'target_side_build_allowed_false'],
  ['productScaffold', 'missiond.product-deployment-closure-preflight.v1'],
  ['productScaffold', 'ReleaseEvidence + ClosureVerdict'],
  ['productScaffold', 'target_side_build_allowed'],
  ['productScaffold', 'db_adoption_plan_missing'],
];

const diagnostics = [];
for (const [id, needle] of required) {
  const file = files[id];
  let text = '';
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (err) {
    diagnostics.push({ file, needle, error: `read failed: ${err.message}` });
    continue;
  }
  if (!text.includes(needle)) diagnostics.push({ file, needle, error: 'missing required deployment closure contract text' });
}

const payload = {
  ok: diagnostics.length === 0,
  schema: 'missiond.deployment-closure-plane-check.v1',
  diagnostics,
};

if (json) console.log(JSON.stringify(payload, null, 2));
else if (payload.ok) console.log('deployment-closure-plane: ok');
else console.error(JSON.stringify(payload, null, 2));

process.exit(payload.ok ? 0 : 1);
