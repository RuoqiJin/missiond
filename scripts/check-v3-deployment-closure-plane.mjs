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
  deployChainAudit: 'scripts/audit-deploy-chain-closure.mjs',
};

const required = [
  ['shard', 'deployment-closure-plane'],
  ['shard', 'ReleaseEvidence'],
  ['shard', 'ClosureVerdict'],
  ['shard', 'ReleaseLease'],
  ['shard', 'ReleasePlan'],
  ['shard', 'RunnerBinding'],
  ['shard', 'SecretRequirement'],
  ['shard', 'gcp-agent MUST NOT claim or execute build_runner jobs'],
  ['shard', 'build_runner_unavailable'],
  ['shard', 'runner_required_env_missing'],
  ['shard', 'deploy-chain-closure-audit'],
  ['shard', 'scripts/audit-deploy-chain-closure.mjs'],
  ['shard', 'pay, search, legal, project-universe, and domain-route closure'],
  ['shard', 'credential presence and Secret Store refs only'],
  ['shard', 'secret-store://missiond/production/MISSIOND_DEPLOY_CENTER_READ_TOKEN'],
  ['shard', 'source freshness MUST be path-aware'],
  ['shard', 'ignoredDirtyLines evidence'],
  ['shard', 'deploy_center_source_relevance_ignored'],
  ['shard', 'compiled Project Universe service config'],
  ['shard', 'domain_management DNS records'],
  ['shard', 'Caddy proxy intent'],
  ['shard', 'compiled_project_universe_unavailable'],
  ['shard', 'service_runtime_config_missing'],
  ['shard', 'deploy_center_slug_mismatch'],
  ['shard', 'target_side_build_not_prohibited'],
  ['shard', 'caddy_proxy_intent_missing'],
  ['shard', 'domain_management_binding_missing'],
  ['shard', 'dns_record_missing'],
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
  ['compiler', 'ReleasePlan'],
  ['compiler', 'RunnerBinding'],
  ['compiler', 'SecretRequirement'],
  ['compiler', 'gcp_build_forbidden'],
  ['compiler', 'build_runner_unavailable'],
  ['compiler', 'runner_required_env_missing'],
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
  ['taskDelegate', 'deploy_ops_mutation_without_approval'],
  ['taskDelegate', 'deploy_ops_action_requires_approval'],
  ['taskDelegate', 'DEPLOY_OPS_MUTATIONS_REQUIRING_APPROVAL'],
  ['taskDelegate', 'mutation_action'],
  ['taskDelegate', 'redeploy'],
  ['serviceLayerTemplate', 'deployment-closure-bundle-standard'],
  ['serviceLayerTemplate', 'scripts/scaffold-product-deployment-closure.mjs'],
  ['serviceLayerTemplate', 'target_side_build_allowed_false'],
  ['productScaffold', 'missiond.product-deployment-closure-preflight.v1'],
  ['productScaffold', 'ReleaseEvidence + ClosureVerdict'],
  ['productScaffold', 'target_side_build_allowed'],
  ['productScaffold', 'db_adoption_plan_missing'],
  ['deployChainAudit', 'missiond.deploy-chain-closure-audit.v1'],
  ['deployChainAudit', 'credentialRefs'],
  ['deployChainAudit', 'DEFAULT_READ_TOKEN_REF'],
  ['deployChainAudit', 'write token Secret Store ref is not declared'],
  ['deployChainAudit', 'deployCenterRepoState'],
  ['deployChainAudit', 'sourceRelevance'],
  ['deployChainAudit', 'ignoredDirtyLines'],
  ['deployChainAudit', 'cargo_diff_does_not_mention_deploy_center'],
  ['deployChainAudit', 'DEPLOY_CENTER_CARGO_RELEVANCE_RE'],
  ['deployChainAudit', 'PROJECT_UNIVERSE_PATH'],
  ['deployChainAudit', 'compiled-project-universe.json'],
  ['deployChainAudit', 'auditConfigClosure'],
  ['deployChainAudit', 'config_gap'],
  ['deployChainAudit', 'service_runtime_config_missing'],
  ['deployChainAudit', 'build_runner_role_missing'],
  ['deployChainAudit', 'runtime_runner_role_missing'],
  ['deployChainAudit', 'target_side_build_not_prohibited'],
  ['deployChainAudit', 'caddy_proxy_intent_missing'],
  ['deployChainAudit', 'domain_management_binding_missing'],
  ['deployChainAudit', 'dns_record_missing'],
  ['deployChainAudit', 'deploy-center-self-update'],
  ['deployChainAudit', 'xjp-payments'],
  ['deployChainAudit', 'xjp-search-center'],
  ['deployChainAudit', 'xjp-legal-service'],
  ['deployChainAudit', 'xjp-project-universe'],
  ['deployChainAudit', 'xjp-domain-service'],
  ['deployChainAudit', 'deploy_center_write_token_missing'],
  ['deployChainAudit', 'deploy_center_provenance_partial'],
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
