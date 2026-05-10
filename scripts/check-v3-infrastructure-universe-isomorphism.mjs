#!/usr/bin/env node

import fs from 'node:fs';

const json = process.argv.includes('--json');

const files = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  frontend: '.missiond/frontend/board-blueprint.lisp',
  infraRs: 'crates/missiond-daemon/src/handlers/sysinfra/infra.rs',
  reconcileRs: 'crates/missiond-daemon/src/handlers/knowledge/project/reconcile.rs',
  mcpInfra: 'crates/missiond-mcp/src/tools/sysinfra/infra.rs',
  frontendRoute: 'packages/board/src/app/api/infra/route.ts',
  frontendSystem: 'packages/board/src/components/SystemDashboard.tsx',
};

const required = [
  ['blueprint', 'infrastructure-universe'],
  ['blueprint', 'runtime-target-contract'],
  ['blueprint', 'credential-ref-contract'],
  ['blueprint', 'skill-evidence-contract'],
  ['blueprint', 'runtime-authority-map'],
  ['blueprint', 'windows-12900kf'],
  ['blueprint', 'privatecloud-hostvds'],
  ['blueprint', 'ecs-pcea'],
  ['blueprint', 'bwg-vps'],
  ['blueprint', 'privatecloud-lan-192-168-1-20'],
  ['blueprint', 'secret-store://deploy-agent/windows-12900kf/agent-token'],
  ['frontend', 'infrastructure-universe'],
  ['frontend', 'packages/board/src/app/api/infra/route.ts'],
  ['infraRs', '"reconcile"'],
  ['infraRs', '"skill_evidence"'],
  ['infraRs', '"credential_refs"'],
  ['infraRs', 'collect_skill_evidence'],
  ['infraRs', 'redact_skill_evidence_line'],
  ['infraRs', 'credentialInlineRisk'],
  ['infraRs', 'privatecloud-hostvds'],
  ['infraRs', 'windows-12900kf'],
  ['reconcileRs', 'runtime_fact_missing'],
  ['reconcileRs', 'credential_inline_risk'],
  ['mcpInfra', '"reconcile"'],
  ['mcpInfra', '"skill_evidence"'],
  ['mcpInfra', '"credential_refs"'],
  ['frontendRoute', "callTool('mission_infra_query'"],
  ['frontendRoute', "action: 'skill_evidence'"],
  ['frontendRoute', "action: 'credential_refs'"],
  ['frontendSystem', 'Infrastructure Universe'],
  ['frontendSystem', 'Credential Refs'],
  ['frontendSystem', 'Runtime Targets'],
];

const diagnostics = [];
const contents = {};
for (const [key, file] of Object.entries(files)) {
  try {
    contents[key] = fs.readFileSync(file, 'utf8');
  } catch (error) {
    diagnostics.push({ file, message: `cannot read: ${error.message}` });
  }
}

if (diagnostics.length === 0) {
  for (const [key, needle] of required) {
    if (!contents[key].includes(needle)) {
      diagnostics.push({ file: files[key], message: `missing required anchor: ${needle}` });
    }
  }
}

const result = {
  ok: diagnostics.length === 0,
  schema: 'missiond.infrastructure-universe-isomorphism-check.v1',
  diagnostics,
};

if (json) {
  console.log(JSON.stringify(result, null, 2));
} else if (result.ok) {
  console.log('v3 infrastructure-universe Lisp/code isomorphism check OK');
} else {
  for (const diagnostic of diagnostics) {
    console.error(`${diagnostic.file}: ${diagnostic.message}`);
  }
}

process.exit(result.ok ? 0 : 1);
