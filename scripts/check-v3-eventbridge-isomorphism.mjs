#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const repo = process.cwd();
const json = process.argv.includes('--json');

const checks = [
  ['.missiond/v3/missiond-blueprint.lisp', ['(eventbridge-policy', '(deployment-event-ingest', '(router-usage-event-ingest', '(deployment-change-classification-policy', '(deploy-agent-self-update-governance', 'deploy_agent_update_provenance', 'missiond.event-envelope.v1', '/webhooks/deploy-center-event', 'agent_offline', 'break-glass-runbook-contract', 'usage_burst', 'provider_auth_failure_burst', 'workflow_run_created', 'workflow_job_succeeded', 'artifact_recorded', 'workflow-only-change', 'validation-only-run', 'local_config_probe', 'typed next_actions', 'runtime env value, webhook reachability, and token-match verification']],
  ['.missiond/workflows/deployment-event-response.lisp', ['deployment-event-response', 'deploy-center', 'no-auto-rollback', 'agent_offline', 'attach-break-glass-runbook', 'task_class=deploy-ops', 'pool_hint=claude-code-deploy-ops', 'raw gh api polling loops are forbidden']],
  ['.missiond/workflows/project-registry-reconciliation.lisp', ['project-registry-reconciliation', 'MissionD', 'deploy-center', 'Forge']],
  ['crates/missiond-core/src/ws/server.rs', ['/webhooks/deploy-center-event', 'require_event_id', '_envelope']],
  ['crates/missiond-daemon/src/bus/bootstrap.rs', ['publish_system_webhook', 'external_service_dedupe_key']],
  ['crates/missiond-daemon/src/bus/v2_subscribers.rs', ['deployment_event_response', 'deployment_event_is_actionable', 'agent_update_failed', 'deployment-event-response', '- task_class: deploy-ops', '- pool_hint: claude-code-deploy-ops', 'xjp_build_wait/xjp_deploy_watch', 'router_event_board_task_input', 'router_event_is_actionable', '- task_class: router-ops', 'provider_auth_failure_burst']],
  ['crates/missiond-daemon/src/handlers/knowledge/context_gather.rs', ['deployment_event_kind_is_relevant', 'workflow_run_created', 'workflow_job_succeeded', 'artifact_recorded', 'accepted_event_kinds', 'observed_candidates', 'relay_diagnostics', 'deploy_center_relay_absent_or_disabled', 'deployment_event_relay_local_config_probe', 'deployment_event_relay_next_actions', 'deploy_center_repo_root_from_service_root', 'local_config_probe', 'checked_missing_relay_env_names', 'DEPLOY_EVENT_RELAY_ENABLED', 'MISSIOND_DEPLOY_EVENT_WEBHOOK_URL', 'Only env variable names and file presence are reported', 'Secret values are not read or emitted', 'runtime env values', 'reachable from the Deploy Center runtime', 'without printing values']],
  ['crates/missiond-daemon/src/handlers/comm/timeline.rs', ['project_id', 'correlation_id', 'external_payload_field_matches']],
  ['crates/missiond-daemon/src/handlers/knowledge/project/reconcile.rs', ['project-registry-reconcile.v1', 'root_mismatch', 'deploy_fact_missing']],
  ['crates/missiond-mcp/src/tools/knowledge/project.rs', ['reconcile', 'deployCenterRoot', 'forgeRoot']],
  ['crates/missiond-mcp/src/tools/comm/timeline.rs', ['projectId', 'correlationId']],
];

const diagnostics = [];
for (const [rel, needles] of checks) {
  const file = path.join(repo, rel);
  if (!fs.existsSync(file)) {
    diagnostics.push({ file: rel, message: 'missing file' });
    continue;
  }
  const text =
    rel === '.missiond/v3/missiond-blueprint.lisp'
      ? readBlueprintWithEvidenceSidecars(repo, rel)
      : fs.readFileSync(file, 'utf8');
  for (const needle of needles) {
    if (!text.includes(needle)) {
      diagnostics.push({ file: rel, message: `missing ${needle}` });
    }
  }
}

const ok = diagnostics.length === 0;
if (json) {
  console.log(JSON.stringify({ ok, diagnostics }, null, 2));
} else if (ok) {
  console.log('v3 eventbridge Lisp/code isomorphism check OK');
} else {
  console.error(`v3 eventbridge Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
}
process.exit(ok ? 0 : 1);
