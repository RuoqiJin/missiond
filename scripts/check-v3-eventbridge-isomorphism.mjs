#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const repo = process.cwd();
const json = process.argv.includes('--json');

const checks = [
  ['.missiond/v3/missiond-blueprint.lisp', ['(eventbridge-policy', '(deployment-event-ingest', '(deploy-agent-self-update-governance', 'deploy_agent_update_provenance', 'missiond.event-envelope.v1', '/webhooks/deploy-center-event']],
  ['.missiond/workflows/deployment-event-response.lisp', ['deployment-event-response', 'deploy-center', 'no-auto-rollback']],
  ['.missiond/workflows/project-registry-reconciliation.lisp', ['project-registry-reconciliation', 'MissionD', 'deploy-center', 'Forge']],
  ['crates/missiond-core/src/ws/server.rs', ['/webhooks/deploy-center-event', 'require_event_id', '_envelope']],
  ['crates/missiond-daemon/src/bus/bootstrap.rs', ['publish_system_webhook', 'external_service_dedupe_key']],
  ['crates/missiond-daemon/src/bus/v2_subscribers.rs', ['deployment_event_response', 'deployment_event_is_actionable', 'agent_update_failed', 'deployment-event-response']],
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
  const text = fs.readFileSync(file, 'utf8');
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
