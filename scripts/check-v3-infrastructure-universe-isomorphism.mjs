#!/usr/bin/env node

import fs from 'node:fs';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const json = process.argv.includes('--json');

const files = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  frontend: '.missiond/frontend/board-blueprint.lisp',
  infraRs: 'crates/missiond-daemon/src/handlers/sysinfra/infra.rs',
  reconcileRs: 'crates/missiond-daemon/src/handlers/knowledge/project/reconcile.rs',
  mcpInfra: 'crates/missiond-mcp/src/tools/sysinfra/infra.rs',
  frontendRoute: 'packages/board/src/app/api/infra/route.ts',
  frontendSystem: 'packages/board/src/components/SystemDashboard.tsx',
  deployDaemon: 'scripts/deploy-daemon.sh',
  macBootstrap: 'scripts/bootstrap-managed-mac-node.sh',
};

const required = [
  ['blueprint', 'infrastructure-universe'],
  ['blueprint', 'runtime-target-contract'],
  ['blueprint', 'credential-ref-contract'],
  ['blueprint', 'skill-evidence-contract'],
  ['blueprint', 'runtime-authority-map'],
  ['blueprint', 'break-glass-runbook-contract'],
  ['blueprint', 'read-only-remote-diagnostic-contract'],
  ['blueprint', 'target-network-profile-contract'],
  ['blueprint', 'artifact-delivery-lane-contract'],
  ['blueprint', 'managed-source-sync-policy'],
  ['blueprint', 'macmini-codebase-local-build-lane'],
  ['blueprint', 'homebrew-managed-toolchain'],
  ['blueprint', 'postgres-client'],
  ['blueprint', 'bootstrap_package_manager'],
  ['blueprint', 'required_diagnostic_clis'],
  ['blueprint', 'postgres_client_package'],
  ['blueprint', 'bootstrap-managed-mac-node.sh'],
  ['blueprint', 'zshenv-managed-block'],
  ['blueprint', 'libpq'],
  ['blueprint', 'psql'],
  ['blueprint', '/opt/homebrew/opt/libpq/bin'],
  ['blueprint', '/usr/local/opt/libpq/bin'],
  ['blueprint', 'GitHub or XJP codebase'],
  ['blueprint', 'rsync/scp'],
  ['blueprint', 'operator-laptop file mirroring'],
  ['blueprint', 'cn-oss-bundle-lane'],
  ['blueprint', 'cloud-registry-lane'],
  ['blueprint', 'gitee-source-mirror-lane'],
  ['blueprint', 'xjp-zibo-lan'],
  ['blueprint', 'privatecloud-10900kf'],
  ['blueprint', 'synology-astrill-gw'],
  ['blueprint', 'ecs-cn-restricted'],
  ['blueprint', 'ss-cn.xiaojinpro.com'],
  ['blueprint', 'diagnostic_profiles'],
  ['blueprint', 'deploy_provenance_snapshot'],
  ['blueprint', 'container_inventory'],
  ['blueprint', 'dependency_manifest_scan'],
  ['blueprint', 'supply_chain_ioc_scan'],
  ['blueprint', 'forbidden_operations'],
  ['blueprint', 'task-result-artifact'],
  ['blueprint', 'agent-offline-response-policy'],
  ['blueprint', 'break_glass_runbook_refs'],
  ['blueprint', 'windows-12900kf'],
  ['blueprint', 'privatecloud-hostvds'],
  ['blueprint', 'ecs-pcea'],
  ['blueprint', 'bwg-vps'],
  ['blueprint', 'privatecloud-lan-192-168-1-20'],
  ['blueprint', 'secret-store-gcp-migration-20260511'],
  ['blueprint', 'ss.xiaojinpro.top'],
  ['blueprint', 'google-cloud-storage'],
  ['blueprint', 'global-object-store'],
  ['blueprint', 'gcp-global-object-store-20260513'],
  ['blueprint', 'cloud-ops-delegation-policy'],
  ['blueprint', 'claude-code-deploy-ops'],
  ['blueprint', 'aliyun-account'],
  ['blueprint', 'aliyun-global'],
  ['blueprint', 'aliyun-dns'],
  ['blueprint', 'changtu-pro-dns'],
  ['blueprint', 'secret-store://aliyun-global/ALIYUN_ACCESS_KEY_ID'],
  ['blueprint', 'secret-store://aliyun-global/ALIYUN_ACCESS_KEY_SECRET'],
  ['blueprint', 'secret-store://secret-store/cloudflare/CLOUDFLARE_DNS_TOKEN'],
  ['blueprint', 'secret-store://deploy-agent/gcp/DEPLOY_AGENT_API_KEY'],
  ['blueprint', 'secret-store://deploy-agent/windows-12900kf/agent-token'],
  ['blueprint', 'secret-store://deploy-agent/DEPLOY_AGENT_ECS_API_KEY'],
  ['frontend', 'infrastructure-universe'],
  ['frontend', 'packages/board/src/app/api/infra/route.ts'],
  ['infraRs', '"reconcile"'],
  ['infraRs', '"skill_evidence"'],
  ['infraRs', '"credential_refs"'],
  ['infraRs', '"diagnostic_profiles"'],
  ['infraRs', 'mission_infra_diagnostic_profiles'],
  ['infraRs', 'diagnostic_profiles('],
  ['infraRs', 'deploy-center-readonly-diagnostic-profile'],
  ['infraRs', 'noRawAgentExecFromMissionD'],
  ['infraRs', 'collect_skill_evidence'],
  ['infraRs', 'redact_skill_evidence_line'],
  ['infraRs', 'credentialInlineRisk'],
  ['infraRs', 'privatecloud-hostvds'],
  ['infraRs', 'windows-12900kf'],
  ['infraRs', 'ss.xiaojinpro.top'],
  ['infraRs', 'secret-store'],
  ['infraRs', 'secret-store://secret-store/cloudflare/CLOUDFLARE_DNS_TOKEN'],
  ['reconcileRs', 'runtime_fact_missing'],
  ['reconcileRs', 'credential_inline_risk'],
  ['mcpInfra', '"reconcile"'],
  ['mcpInfra', '"skill_evidence"'],
  ['mcpInfra', '"credential_refs"'],
  ['mcpInfra', '"diagnostic_profiles"'],
  ['frontendRoute', "callTool('mission_infra_query'"],
  ['frontendRoute', "action: 'skill_evidence'"],
  ['frontendRoute', "action: 'credential_refs'"],
  ['frontendRoute', "action: 'diagnostic_profiles'"],
  ['frontendSystem', 'Infrastructure Universe'],
  ['frontendSystem', 'Credential Refs'],
  ['frontendSystem', 'Read-only Diagnostic Profiles'],
  ['frontendSystem', 'Runtime Targets'],
  ['deployDaemon', '/opt/homebrew/opt/libpq/bin'],
  ['deployDaemon', '/usr/local/opt/libpq/bin'],
  ['macBootstrap', 'install Homebrew'],
  ['macBootstrap', 'brew install libpq'],
  ['macBootstrap', 'psql --version'],
];

const diagnostics = [];
const contents = {};
for (const [key, file] of Object.entries(files)) {
  try {
    contents[key] = key === 'blueprint'
      ? readBlueprintWithEvidenceSidecars(process.cwd(), file)
      : fs.readFileSync(file, 'utf8');
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
