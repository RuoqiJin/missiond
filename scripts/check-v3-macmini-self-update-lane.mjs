#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const args = new Set(process.argv.slice(2));
const json = args.has('--json');
const repoRoot = process.cwd();

const FILES = {
  workflow: '.missiond/workflows/missiond-macmini-self-update.lisp',
  infrastructure: '.missiond/v3/shards/universe/infrastructure.lisp',
  deploymentRollout: '.missiond/workflows/m6-deployment-rollout.lisp',
  nativeRunnerWorkflow: '.missiond/workflows/xjp-native-codebase-runner-convergence.lisp',
  jarvisInteractionSmoke: 'scripts/smoke-jarvis-interaction.mjs',
  jarvisChainSmoke: 'scripts/smoke-jarvis-chain.mjs',
};

const diagnostics = [];
const sources = {};

for (const [key, rel] of Object.entries(FILES)) {
  const abs = path.join(repoRoot, rel);
  try {
    sources[key] = fs.readFileSync(abs, 'utf8');
  } catch (error) {
    diagnostics.push({ file: rel, code: 'MISSING_FILE', message: `cannot read ${rel}: ${error.message}` });
  }
}

if (diagnostics.length === 0) {
  requireAll(FILES.workflow, sources.workflow, [
    ':workflow_id missiond-macmini-self-update',
    ':status active',
    ':source_plans [interaction-gateway work-order-lifecycle xjp-native-codebase-runner-convergence m6-deployment-rollout]',
    '(trigger-kind version_bump_push)',
    '(target_id rickyhq-macmini-m4)',
    '(executor_name macmini)',
    '(source_sync_provider github)',
    '(target_root "/Users/rickyhq/Projects/missiond")',
    '(deploy_command "scripts/deploy-daemon.sh --debug")',
    ':id receive-client-objective',
    ':id draft-deployment-plan',
    ':id create-native-workflow-run',
    ':id macmini-fetch-source',
    ':id verify-commit-version',
    ':id target-build-test',
    ':id blue-green-deploy',
    ':id monitor-smoke',
    ':id publish-provenance',
    'services/deploy-center/scripts/run-missiond-macmini-self-update.sh',
    'git pull --ff-only',
    'scripts/deploy-daemon.sh --debug',
    'http://127.0.0.1:9120/internal/jarvis/slot/ensure',
    'http://127.0.0.1:9120/api/monitor/jarvis',
    'no-rsync-scp',
    'client-channel-required',
    'task-result-artifact-required',
    'rollback-artifact-required',
  ]);

  requireAll(FILES.infrastructure, sources.infrastructure, [
    'rickyhq-macmini-m4',
    ':deploy_center_executor macmini',
    '/Users/rickyhq/Projects/missiond',
    '/Users/rickyhq/.xjp-mission',
    'macmini-codebase-local-build-lane',
  ]);

  requireAll(FILES.deploymentRollout, sources.deploymentRollout, [
    'For managed build targets such as rickyhq Mac mini',
    'sync source through GitHub or XJP codebase/deploy-center CodebaseSyncOperation',
    'build/test/install on the target node',
    'Operator-laptop rsync/scp is break-glass only',
  ]);

  requireAll(FILES.nativeRunnerWorkflow, sources.nativeRunnerWorkflow, [
    'For managed Mac nodes such as rickyhq-macmini-m4',
    'XJP codebase sync plus on-target cargo build/test/install',
    'ad-hoc rsync/ssh from an operator laptop is break-glass only',
  ]);

  requireAll(FILES.jarvisInteractionSmoke, sources.jarvisInteractionSmoke, [
    'missiond.jarvis-interaction-smoke.v1',
    'INTERACTION_AUTH_REQUIRED',
    'intent_draft',
    'confirm_required',
  ]);

  requireAll(FILES.jarvisChainSmoke, sources.jarvisChainSmoke, [
    'jarvis-chain',
    '/api/monitor/jarvis',
    'overall',
  ]);
}

const result = {
  ok: diagnostics.length === 0,
  schema: 'missiond.macmini-self-update-lane-check.v1',
  diagnostics,
};

if (json) {
  console.log(JSON.stringify(result, null, 2));
} else if (result.ok) {
  console.log('MissionD Mac mini self-update lane check OK');
} else {
  for (const diagnostic of diagnostics) {
    console.error(`${diagnostic.file}: ${diagnostic.code}: ${diagnostic.message}`);
  }
}

process.exit(result.ok ? 0 : 1);

function requireAll(file, source, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) {
      diagnostics.push({
        file,
        code: 'MISSING_TOKEN',
        message: `missing token: ${token}`,
      });
    }
  }
}
