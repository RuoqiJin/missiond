#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const repo = process.cwd();
const json = process.argv.includes('--json');
const xjpRoot = '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend';

const checks = [
  [
    '.missiond/v3/missiond-blueprint.lisp',
    [
      '(memory-provider-contract',
      ':schema "missiond.memory-provider.v1"',
      ':providers',
      '(provider null-memory',
      '(provider local-postgres-memory',
      '(provider xjp-memory',
      ':scope-fields [tenant_id universe_id project_id user_id source_type source_id authority privacy_class review_state]',
      'MissionD Core MUST NOT require private memory data',
      'mission_kb_query and mission_kb_remember are compatibility leaves',
      '(eventhub-service-contract',
      ':schema "missiond.eventhub-service-contract.v1"',
      ':service-id xjp-eventhub',
      'MissionD local EventBus remains the low-latency agent/Board/slot/workflow control bus',
      'xjp-eventhub is the durable cross-service event backbone',
      '(provider-runtime-bringup-contract',
      ':schema "missiond.provider-runtime-bringup.v1"',
      'scripts/manage-local-providers.sh',
      'local-provider-launchd',
      'local-provider-smoke',
      'com.xjp.memory.provider',
      'com.xjp.eventhub.provider',
      'postgres-durable',
      '(domain memory-access-plane',
      '(domain eventhub-extraction-plane',
      '(surface memory-provider-boundary',
      ':implements [memory-provider-contract memory-provider]',
      '(surface eventhub-service-boundary',
      ':implements [eventhub-service-contract eventhub-service]',
      '(function memory-provider',
      '(function eventhub-service',
      'local-event-spool',
      'MemoryProviderConfig',
      'EventHubClient',
      'node scripts/check-v3-service-extraction-isomorphism.mjs',
    ],
  ],
  [
    '.missiond/workflows/missiond-module-extraction.lisp',
    [
      '(workflow missiond-module-extraction',
      ':workflow_id missiond-module-extraction',
      ':status active',
      ':source_plans [memory-provider-contract eventhub-service-contract control-plane-m6-split]',
      ':id',
      'review-boundary',
      'pin-provider-contract',
      'pin-eventhub-contract',
      'add-compatibility-facades',
      'scaffold-services',
      'dual-read-write',
      'cutover',
      'Do not physical-delete existing KB/conversation/event tables',
      'Do not make xjp-eventhub required for local Board/workstation/autopilot execution',
    ],
  ],
  [
    '.missiond/research/missiond-memory-eventhub-modularization-20260512.md',
    [
      'Memory Provider + EventHub Modularization Decision',
      'null-memory',
      'local-postgres-memory',
      'xjp-memory',
      'xjp-eventhub',
      'local EventBus',
      'compatibility adapters',
    ],
  ],
  [
    'scripts/check-v3-code-isomorphism-complete.mjs',
    [
      "'memory-provider-boundary'",
      "'eventhub-service-boundary'",
      "'scripts/check-v3-service-extraction-isomorphism.mjs'",
    ],
  ],
  [
    'scripts/check-v3-control-plane-m6-split.mjs',
    [
      'eventhub-extraction-plane',
      'memory-access-plane',
      'eventhub-service-contract',
      'memory-provider-contract',
    ],
  ],
];

const diagnostics = [];

function checkFile(base, rel, needles) {
  const file = path.join(base, rel);
  if (!fs.existsSync(file)) {
    diagnostics.push({ file, message: 'missing file' });
    return;
  }
  const text = fs.readFileSync(file, 'utf8');
  for (const needle of needles) {
    if (!text.includes(needle)) {
      diagnostics.push({ file, message: `missing ${needle}` });
    }
  }
}

for (const [rel, needles] of checks) {
  checkFile(repo, rel, needles);
}

const xjpChecks = [
  [
    'Cargo.toml',
    [
      '"services/xjp-memory"',
      '"services/xjp-eventhub"',
    ],
  ],
  [
    '.missiond/intent.lisp',
    [
      '(service xjp-memory',
      '(service xjp-eventhub',
      ':path "services/xjp-memory"',
      ':path "services/xjp-eventhub"',
    ],
  ],
  [
    'services.yaml',
    [
      'xjp-memory:',
      'xjp-eventhub:',
      'port: 8091',
      'port: 8092',
      '/v1/memory/*',
      '/v1/eventhub/*',
      'dc_slug: xjp-memory',
      'dc_slug: xjp-eventhub',
    ],
  ],
  [
    '.missiond/backend/xiaojinpro-backend-blueprint.lisp',
    [
      '"services/xjp-memory"',
      '"services/xjp-eventhub"',
      'xjp-memory=8091',
      'xjp-eventhub=8092',
      '(shard xjp-memory',
      '(shard xjp-eventhub',
    ],
  ],
  [
    '.missiond/check.sh',
    [
      'services/xjp-memory/.missiond/intent.lisp',
      'services/xjp-eventhub/.missiond/intent.lisp',
      'services/xjp-memory/.missiond/check.sh',
      'services/xjp-eventhub/.missiond/check.sh',
    ],
  ],
  [
    'scripts/check-xjp-ssot-complete.mjs',
    [
      "id: 'xjp-memory'",
      "id: 'xjp-eventhub'",
      "root: 'services/xjp-memory'",
      "root: 'services/xjp-eventhub'",
    ],
  ],
  [
    '.missiond/evidence/current-code-mapping.md',
    [
      'xjp-memory=8091',
      'xjp-eventhub=8092',
      'M6 durable service extractions',
    ],
  ],
  [
    'services/xjp-memory/Cargo.toml',
    [
      'name = "xjp-memory"',
      'anyhow = { workspace = true }',
    ],
  ],
  [
    'services/xjp-memory/src/main.rs',
    [
      'std::env::var("PORT")',
      'unwrap_or(8091)',
      'create_simple_health_routes',
      'xjp_memory::bootstrap_from_env',
      'xjp_memory::create_router',
    ],
  ],
  [
    'services/xjp-memory/src/lib.rs',
    [
      '/v1/memory/provider_status',
      '/v1/memory/query',
      '/v1/memory/remember',
      '/v1/memory/review',
      'MemoryScope',
      'ProviderStatus',
    ],
  ],
  [
    'services/xjp-memory/.missiond/intent.lisp',
    [
      '(intent xjp-memory',
      ':maturity-current M6',
      ':maturity-target M6',
      'memory-provider',
    ],
  ],
  [
    'services/xjp-memory/.missiond/backend/xjp-memory-backend-blueprint.lisp',
    [
      '(blueprint xjp-memory-backend',
      '(pillar memory-provider',
      'provider-status',
      'scoped-query',
      ':entry',
      ':core',
      ':egress',
      ':surfaces',
    ],
  ],
  [
    'services/xjp-memory/.missiond/evidence/current-code-mapping.md',
    [
      '/v1/memory/provider_status',
      '/v1/memory/query',
      '/v1/memory/remember',
      '/v1/memory/review',
    ],
  ],
  [
    'services/xjp-memory/.missiond/check.sh',
    [
      '[xjp-memory-check]',
    ],
  ],
  [
    'services/xjp-eventhub/Cargo.toml',
    [
      'name = "xjp-eventhub"',
      'anyhow = { workspace = true }',
    ],
  ],
  [
    'services/xjp-eventhub/src/main.rs',
    [
      'std::env::var("PORT")',
      'unwrap_or(8092)',
      'create_simple_health_routes',
      'xjp_eventhub::bootstrap_from_env',
      'xjp_eventhub::create_router',
    ],
  ],
  [
    'services/xjp-eventhub/src/lib.rs',
    [
      '/v1/eventhub/status',
      '/v1/eventhub/events',
      '/v1/eventhub/query',
      'missiond.event-envelope.v1',
      'idempotency',
    ],
  ],
  [
    'services/xjp-eventhub/.missiond/intent.lisp',
    [
      '(intent xjp-eventhub',
      ':maturity-current M6',
      ':maturity-target M6',
      'eventhub-service',
    ],
  ],
  [
    'services/xjp-eventhub/.missiond/backend/xjp-eventhub-backend-blueprint.lisp',
    [
      '(blueprint xjp-eventhub-backend',
      '(pillar eventhub',
      'append-event-envelope',
      'query-event-envelope',
      ':entry',
      ':core',
      ':egress',
      ':surfaces',
    ],
  ],
  [
    'services/xjp-eventhub/.missiond/evidence/current-code-mapping.md',
    [
      '/v1/eventhub/status',
      '/v1/eventhub/events',
      '/v1/eventhub/query',
    ],
  ],
  [
    'services/xjp-eventhub/.missiond/check.sh',
    [
      '[xjp-eventhub-check]',
    ],
  ],
];

for (const [rel, needles] of xjpChecks) {
  checkFile(xjpRoot, rel, needles);
}

const localProviderChecks = [
  [
    'scripts/manage-local-providers.sh',
    [
      'com.xjp.memory.provider',
      'com.xjp.eventhub.provider',
      'MISSIOND_MEMORY_PROVIDER_URL',
      'MISSIOND_EVENTHUB_URL',
      'MISSIOND_MEMORY_PROVIDER_MODE',
      'MISSIOND_EVENTHUB_MODE',
      'XJP_MEMORY_DATABASE_URL',
      'XJP_EVENTHUB_DATABASE_URL',
      'XJP_MEMORY_PROVIDER_ID',
      'XJP_EVENTHUB_PROVIDER_ID',
      'xjp_memory',
      'xjp_eventhub',
      '/v1/memory/provider_status',
      '/v1/eventhub/status',
      'launchctl bootstrap',
      'postgres-durable',
    ],
  ],
];

for (const [rel, needles] of localProviderChecks) {
  checkFile(repo, rel, needles);
}

const ok = diagnostics.length === 0;
if (json) {
  console.log(JSON.stringify({ ok, diagnostics }, null, 2));
} else if (ok) {
  console.log('v3 service-extraction boundary check OK');
} else {
  console.error(`v3 service-extraction boundary check FAILED -- ${diagnostics.length} diagnostic(s)`);
  for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
}
process.exit(ok ? 0 : 1);
