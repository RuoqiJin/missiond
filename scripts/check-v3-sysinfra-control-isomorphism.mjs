#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-sysinfra-control-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 sysinfra-control Lisp/code isomorphism contract:
  - V2 sysinfra/permission/power/global-instruction design is promoted to V3.
  - Public sysinfra MCP tools route through dedicated daemon modules.
  - MCP schemas expose the daemon-supported actions, including merged_for_slot.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  dispatcher: 'crates/missiond-daemon/src/handlers/mod.rs',
  sysinfraMod: 'crates/missiond-daemon/src/handlers/sysinfra/mod.rs',
  infra: 'crates/missiond-daemon/src/handlers/sysinfra/infra.rs',
  permission: 'crates/missiond-daemon/src/handlers/sysinfra/permission.rs',
  learnedPermissions: 'crates/missiond-core/src/core/learned_permissions.rs',
  power: 'crates/missiond-daemon/src/handlers/sysinfra/power.rs',
  system: 'crates/missiond-daemon/src/handlers/sysinfra/system.rs',
  globalInstruction: 'crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs',
  mcpInfra: 'crates/missiond-mcp/src/tools/sysinfra/infra.rs',
  mcpPermission: 'crates/missiond-mcp/src/tools/sysinfra/permission.rs',
  mcpPower: 'crates/missiond-mcp/src/tools/sysinfra/power.rs',
  mcpSystem: 'crates/missiond-mcp/src/tools/sysinfra/system.rs',
  mcpGlobalInstruction: 'crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs',
  deployScript: 'scripts/deploy-daemon.sh',
  v2Tools: '.missiond/v2/intent-tools.lisp',
  v2Flow: '.missiond/v2/intent-flow.lisp',
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
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 sysinfra-control Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 sysinfra-control Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }

  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'sysinfra-control',
    '(v2-item sysinfra-control',
    ':status code-aligned',
    '(tool-group sysinfra-control-tools',
    '(surface sysinfra-control',
    ':status "code-aligned"',
    'mission_infra_query',
    'mission_infra_ops',
    'mission_permission_query',
    'mission_permission_mutate',
    'mission_power_control',
    'mission_sys_logs',
    'mission_sys_config',
    'mission_daemon_update',
    'mission_global_instruction',
    'crates/missiond-daemon/src/handlers/sysinfra/infra.rs',
    'crates/missiond-daemon/src/handlers/sysinfra/permission.rs',
    'crates/missiond-daemon/src/handlers/sysinfra/power.rs',
    'crates/missiond-daemon/src/handlers/sysinfra/system.rs',
    'crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs',
    'crates/missiond-mcp/src/tools/sysinfra/permission.rs',
    'scripts/check-v3-sysinfra-control-isomorphism.mjs',
    'infra.rs owns mission_infra_query/ops',
    'skill-derived infra facts',
    'windows-runner/12900kf',
    'agent_url=windows',
    'permission.rs owns mission_permission_query/mutate',
    'power.rs owns mission_power_control',
    'system.rs owns mission_sys_logs, mission_sys_config, mission_daemon_update, and missiond-blue-green-self-update',
    'mission_daemon_update full build MUST start scripts/deploy-daemon.sh as a detached async logged job',
    'require explicit confirm=true',
    'deploy-daemon.sh MUST co-build missiond and mission-mcp into one blue-green release',
    'missiond-blue-green-self-update',
    'infrastructure-universe',
    'runtime-target-contract',
    'skill-evidence-contract',
    'credential-ref-contract',
    'survive daemon kickstart',
    'skip_build remains the synchronous already-built artifact restart path',
    'global_instruction.rs owns mission_global_instruction',
    'node scripts/check-v3-sysinfra-control-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.dispatcher, sources.dispatcher, [
    'use sysinfra::{global_instruction, health, infra, misc, permission, power, system};',
    '"mission_infra_query" | "mission_infra_ops" => infra::handle',
    '"mission_permission_query" | "mission_permission_mutate"',
    '"mission_sys_logs" | "mission_sys_config" | "mission_daemon_update"',
    '"mission_power_control" => power::handle',
    '"mission_global_instruction" => global_instruction::handle',
    'n.starts_with("mission_infra_")',
    'n.starts_with("mission_permission_")',
    'n == "mission_health"',
  ]);

  requireAll(diagnostics, files.sysinfraMod, sources.sysinfraMod, [
    'pub(crate) mod global_instruction;',
    'pub(crate) mod infra;',
    'pub(crate) mod permission;',
    'pub(crate) mod power;',
    'pub(crate) mod system;',
  ]);

  requireAll(diagnostics, files.infra, sources.infra, [
    'mission_infra_query',
    'mission_infra_ops',
    '"list"',
    '"get"',
    '"reconcile"',
    '"skill_evidence"',
    '"credential_refs"',
    '"health"',
    '"reachability"',
    '"diagnose"',
    'mission_infra_list',
    'mission_infra_get',
    'mission_reachability',
    'mission_os_diagnose',
    'skill_derived',
    'collect_skill_evidence',
    'redact_skill_evidence_line',
    'credentialInlineRisk',
    'privatecloud-hostvds',
    'WINDOWS_12900KF_SKILL',
    'WINDOWS_12900KF_INFRA_ID',
    'windows-runner',
    '12900kf',
    'agent_url=windows',
    'parse_ssh_targets',
    'tailscale',
    'deploy_agent',
  ]);

  requireAll(diagnostics, files.mcpInfra, sources.mcpInfra, [
    '"reconcile"',
    '"skill_evidence"',
    '"credential_refs"',
    'secret_ref',
  ]);

  requireAll(diagnostics, files.permission, sources.permission, [
    'mission_permission_query',
    'mission_permission_mutate',
    '"get"',
    '"learned_list"',
    '"merged_for_slot"',
    '"set_role"',
    '"set_slot"',
    '"auto_allow"',
    '"reload"',
    '"revoke"',
    'mission_permission_merged_for_slot',
    'learned.get_for_spawn',
    'get_role_rule',
    'get_slot_rule',
  ]);
  requireAll(diagnostics, files.learnedPermissions, sources.learnedPermissions, [
    'expires_at',
    'source_evidence',
    'renew_policy',
    'audit_trail',
    'DEFAULT_PERMISSION_TTL_DAYS',
    'is_expired_permission',
    'use-renews',
    'provider-confirmation',
  ]);

  requireAll(diagnostics, files.power, sources.power, [
    'mission_power_control',
    '"status"',
    '"wake"',
    '"suspend"',
    'tokio::net::TcpStream::connect',
    'Power control: wake requested',
    'Power control: suspend requested',
  ]);

  requireAll(diagnostics, files.system, sources.system, [
    'mission_sys_logs',
    'mission_sys_config',
    'mission_daemon_update',
    'ALLOWED_CONFIGS',
    'resolve_config_path',
    'sys_config_patch',
    'find_latest_log',
    'daemon_update',
    'LAUNCHD_LABEL',
    'current_exe',
    'scripts/deploy-daemon.sh',
    'daemon-update-',
    'async deploy job started',
    'Stdio::null',
    'setsid',
    'pre_exec',
    'codesign',
    'launchctl',
    'missiond-update.sh',
  ]);

  requireAll(diagnostics, files.deployScript, sources.deployScript, [
    'MISSIOND_MCP_BIN_PATH',
    'MISSIOND_RELEASES_DIR',
    'MISSIOND_ACTIVE_LINK',
    'cargo build ${BUILD_ARG} -p missiond-daemon -p missiond-mcp',
    'MCP_ARTIFACT',
    'release-manifest.json',
    'codesign_or_verify "$CANDIDATE_DIR/bin/mission-mcp"',
    'switch_active_release',
    'rollback_to_previous',
    'cleanup_old_releases',
    '$ACTIVE_LINK/bin/mission-mcp',
  ]);

  requireAll(diagnostics, files.globalInstruction, sources.globalInstruction, [
    'mission_global_instruction',
    'global_claude_md_path',
    '"read"',
    '"edit"',
    '"reload"',
    'read_action',
    'edit_action',
    'reload_action',
    'dry_run',
    'allow_empty',
    'backup_path',
    'manual-reload-required',
    'structured_error',
  ]);

  requireAll(diagnostics, files.mcpInfra, sources.mcpInfra, [
    'mission_infra_query',
    'mission_infra_ops',
    '"health"',
    '"reachability"',
    '"diagnose"',
  ]);
  requireAll(diagnostics, files.mcpPermission, sources.mcpPermission, [
    'mission_permission_query',
    'mission_permission_mutate',
    '"get"',
    '"learned_list"',
    '"merged_for_slot"',
    '"slotId"',
    '"set_role"',
    '"set_slot"',
    '"auto_allow"',
    '"reload"',
    '"revoke"',
  ]);
  requireAll(diagnostics, files.mcpPower, sources.mcpPower, [
    'mission_power_control',
    '"wake"',
    '"suspend"',
    '"status"',
  ]);
  requireAll(diagnostics, files.mcpSystem, sources.mcpSystem, [
    'mission_sys_logs',
    'mission_sys_config',
    'mission_daemon_update',
    'slots.yaml',
    'llm.yaml',
    'permissions.yaml',
  ]);
  requireAll(diagnostics, files.mcpGlobalInstruction, sources.mcpGlobalInstruction, [
    'mission_global_instruction',
    '"read"',
    '"edit"',
    '"reload"',
    'manual-reload-required',
  ]);

  requireAll(diagnostics, files.v2Tools, sources.v2Tools, [
    'mission_permission_query',
    'merged_for_slot',
    'mission_power_control',
    'mission_sys_config',
    'mission_daemon_update',
    'mission_global_instruction',
  ]);
  requireAll(diagnostics, files.v2Flow, sources.v2Flow, [
    'mission_infra_ops',
    'mission_power_control',
    'mission_permission_query',
    'mission_global_instruction',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, rel, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file: rel, message: `missing required text: ${needle}` });
    }
  }
}

function ensureFile(root, rel, text = '') {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text);
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-sysinfra-control-'));
  const blueprint = `(missiond-blueprint-v3
  (v2-convergence-map
    (v2-item sysinfra-control :status code-aligned))
  (public-surface-map
    (tool-group sysinfra-control-tools
      :status code-aligned
      :tools [mission_infra_query mission_infra_ops mission_permission_query mission_permission_mutate
              mission_power_control mission_sys_logs mission_sys_config mission_daemon_update
              mission_global_instruction]))
  (implementation-map
    (surface sysinfra-control
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/permission.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/power.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"
             "crates/missiond-mcp/src/tools/sysinfra/permission.rs"
             "scripts/check-v3-sysinfra-control-isomorphism.mjs"]
      :note "infra.rs owns mission_infra_query/ops; permission.rs owns mission_permission_query/mutate; power.rs owns mission_power_control; system.rs owns mission_sys_logs, mission_sys_config, mission_daemon_update, and missiond-blue-green-self-update. mission_daemon_update must require explicit confirm=true; full build MUST start scripts/deploy-daemon.sh as a detached async logged job to survive daemon kickstart; deploy-daemon.sh MUST co-build missiond and mission-mcp into one blue-green release; skip_build remains the synchronous already-built artifact restart path. global_instruction.rs owns mission_global_instruction."))
  (compression-contract
    :checks ["node scripts/check-v3-sysinfra-control-isomorphism.mjs"]))`;
  ensureFile(root, DEFAULT_FILES.blueprint, blueprint);

  const common = 'mission_infra_query mission_infra_ops mission_permission_query mission_permission_mutate mission_power_control mission_sys_logs mission_sys_config mission_daemon_update mission_global_instruction "read" "edit" "reload" "list" "get" "learned_list" "merged_for_slot" "slotId" "set_role" "set_slot" "auto_allow" "reload" "revoke" "wake" "suspend" "status" "health" "reachability" "diagnose" slots.yaml llm.yaml permissions.yaml manual-reload-required';
  for (const rel of Object.values(DEFAULT_FILES)) {
    if (rel === DEFAULT_FILES.blueprint) continue;
    ensureFile(root, rel, common);
  }
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.dispatcher),
    ' use sysinfra::{global_instruction, health, infra, misc, permission, power, system}; "mission_infra_query" | "mission_infra_ops" => infra::handle "mission_permission_query" | "mission_permission_mutate" "mission_sys_logs" | "mission_sys_config" | "mission_daemon_update" "mission_power_control" => power::handle "mission_global_instruction" => global_instruction::handle n.starts_with("mission_infra_") n.starts_with("mission_permission_") n == "mission_health"',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.sysinfraMod),
    ' pub(crate) mod global_instruction; pub(crate) mod infra; pub(crate) mod permission; pub(crate) mod power; pub(crate) mod system;',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.infra),
    ' mission_infra_list mission_infra_get mission_reachability mission_os_diagnose parse_ssh_targets tailscale deploy_agent',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.permission),
    ' mission_permission_merged_for_slot learned.get_for_spawn get_role_rule get_slot_rule',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.learnedPermissions),
    ' expires_at source_evidence renew_policy audit_trail DEFAULT_PERMISSION_TTL_DAYS is_expired_permission use-renews provider-confirmation',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.power),
    ' tokio::net::TcpStream::connect Power control: wake requested Power control: suspend requested',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.system),
    ' ALLOWED_CONFIGS resolve_config_path sys_config_patch find_latest_log daemon_update LAUNCHD_LABEL current_exe scripts/deploy-daemon.sh daemon-update- async deploy job started Stdio::null setsid pre_exec codesign launchctl missiond-update.sh',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.deployScript),
    ' MISSIOND_MCP_BIN_PATH MISSIOND_RELEASES_DIR MISSIOND_ACTIVE_LINK cargo build ${BUILD_ARG} -p missiond-daemon -p missiond-mcp MCP_ARTIFACT release-manifest.json codesign_or_verify "$CANDIDATE_DIR/bin/mission-mcp" switch_active_release rollback_to_previous cleanup_old_releases $ACTIVE_LINK/bin/mission-mcp',
  );
  fs.appendFileSync(
    path.join(root, DEFAULT_FILES.globalInstruction),
    ' global_claude_md_path read_action edit_action reload_action dry_run allow_empty backup_path structured_error',
  );

  return root;
}

main();
