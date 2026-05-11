#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-capability-governance-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 capability-governance Lisp/code isomorphism contract:
  - mission_capability_usage is exposed through a thin facade and pinned runtime.
  - mission_audit and mission_codex_ops stay in the same governance surface.
  - MCP schemas, daemon dispatch, and V3 blueprint agree on the public tools.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  dispatcher: 'crates/missiond-daemon/src/handlers/mod.rs',
  capabilityFacade: 'crates/missiond-daemon/src/handlers/comm/capability_usage.rs',
  capabilityRuntime: 'crates/missiond-daemon/src/handlers/comm/capability_usage/runtime.rs',
  v3Runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  audit: 'crates/missiond-daemon/src/handlers/comm/audit.rs',
  codexOps: 'crates/missiond-daemon/src/handlers/comm/codex_ops.rs',
  toolDirectory: 'crates/missiond-daemon/src/handlers/comm/tool_directory.rs',
  mcpCapability: 'crates/missiond-mcp/src/tools/comm/capability_usage.rs',
  mcpAudit: 'crates/missiond-mcp/src/tools/comm/audit.rs',
  mcpCodexOps: 'crates/missiond-mcp/src/tools/comm/codex_ops.rs',
  mcpToolDirectory: 'crates/missiond-mcp/src/tools/comm/tool_directory.rs',
  v2Source: '.missiond/v2/intent-capability-governance.lisp',
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
    console.log('v3 capability-governance Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 capability-governance Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
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
    'capability-governance',
    '(v2-item capability-governance',
    ':status runtime-projected',
    '(capability-governance-policy',
    '(mcp-tool-governance-policy',
    ':primary-families',
    ':directory-tool mission_tool_directory',
    ':review-sidecar ".missiond/v3/runtime/capability-usage-review.json"',
    ':protected-tool-patterns',
    ':protected-flow-patterns',
    '(tool-group capability-audit-tools',
    '(tool-group mcp-tool-governance-tools',
    'mission_tool_directory',
    '(surface capability-governance',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
    'crates/missiond-daemon/src/handlers/comm/capability_usage.rs',
    'crates/missiond-daemon/src/handlers/comm/capability_usage/runtime.rs',
    'crates/missiond-daemon/src/handlers/comm/audit.rs',
    'crates/missiond-daemon/src/handlers/comm/codex_ops.rs',
    'crates/missiond-daemon/src/handlers/comm/tool_directory.rs',
    'crates/missiond-mcp/src/tools/comm/capability_usage.rs',
    'crates/missiond-mcp/src/tools/comm/audit.rs',
    'crates/missiond-mcp/src/tools/comm/codex_ops.rs',
    'crates/missiond-mcp/src/tools/comm/tool_directory.rs',
    'scripts/check-v3-capability-governance-isomorphism.mjs',
    'capability_usage.rs is the thin capability-governance facade',
    'capability_usage/runtime.rs owns snapshot/report/candidates/mark/ack',
    'audit.rs owns mission_audit trace/detail/stats/export',
    'codex_ops.rs owns mission_codex_ops recent/thread/tool_stats',
    'tool_directory.rs owns mission_tool_directory list/recommend/lookup/explain/deprecated',
    'node scripts/check-v3-capability-governance-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.dispatcher, sources.dispatcher, [
    '"mission_capability_usage" => capability_usage::handle',
    '"mission_tool_directory" => tool_directory::handle',
    '"mission_audit" => audit::handle',
    '"mission_codex_ops" => codex_ops::handle',
    'n.starts_with("mission_audit_") => audit::handle',
  ]);

  requireAll(diagnostics, files.capabilityFacade, sources.capabilityFacade, [
    'mod runtime;',
    'runtime::handle(state, name, args).await',
    'Thin V3 facade for the capability-governance read model',
  ]);

  requireAll(diagnostics, files.capabilityRuntime, sources.capabilityRuntime, [
    'mission_capability_usage',
    'snapshot|report|candidates|mark|ack',
    'CapabilityUsageSnapshot',
    'CapabilityStaleCandidate',
    'CapabilityGovernanceRuntimeConfig',
    'load_governance_config',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'LEGACY_CAPABILITY_REVIEW_SIDECAR',
    'review_sidecar_path',
    'protected_tool_patterns',
    'protected_flow_patterns',
    'action_snapshot',
    'action_report',
    'action_candidates',
    'action_mark',
    'action_ack',
    'source_coverage',
    'conversation_tool_calls',
    'board_tasks_flow_template',
    'event_log_flow_events',
    'lisp_semantic_hints',
    'review_sidecar',
    'workflow_execution_stats',
    'compute_protected_target_policy',
    'compute_review_required',
    'pick_merge_target',
    'apply_hint_overlay',
    'observability',
  ]);

  requireAll(diagnostics, files.v3Runtime, sources.v3Runtime, [
    'CapabilityGovernanceRuntimeConfig',
    'parse_capability_governance_policy',
    'DEFAULT_CAPABILITY_REVIEW_SIDECAR',
    'DEFAULT_PROTECTED_TOOL_PATTERNS',
    'DEFAULT_PROTECTED_FLOW_PATTERNS',
    'capability-governance-policy',
    ':review-sidecar',
    ':protected-tool-patterns',
    ':protected-flow-patterns',
  ]);

  requireAll(diagnostics, files.audit, sources.audit, [
    'mission_audit',
    'mission_audit_trace',
    'mission_audit_detail',
    'mission_audit_stats',
    'mission_audit_export',
    'format_tool_call_trace',
    'get_tool_calls_by_session',
    'get_tool_call_by_id',
    'get_board_task_notes',
  ]);

  requireAll(diagnostics, files.codexOps, sources.codexOps, [
    'mission_codex_ops',
    'handle_recent',
    'handle_thread',
    'handle_tool_stats',
    'parse_since_to_utc',
    'codex_cli',
    'get_tool_calls_by_session',
    'get_tool_call_stats',
  ]);

  requireAll(diagnostics, files.toolDirectory, sources.toolDirectory, [
    'mission_tool_directory',
    'ToolFamily',
    'FAMILIES',
    'recommend',
    'lookup_tool',
    'deprecated',
    'mission_board',
    'mission_workflow',
    'mission_workstation',
    'mission_context',
    'mission_memory',
    'mission_universe',
    'mission_ops',
    'mission_router',
  ]);

  requireAll(diagnostics, files.mcpCapability, sources.mcpCapability, [
    'ToolDefinition::new',
    '"mission_capability_usage"',
    '"snapshot"',
    '"report"',
    '"candidates"',
    '"mark"',
    '"ack"',
  ]);

  requireAll(diagnostics, files.mcpAudit, sources.mcpAudit, [
    'ToolDefinition::new',
    '"mission_audit"',
    '"trace"',
    '"detail"',
    '"stats"',
    '"export"',
  ]);

  requireAll(diagnostics, files.mcpCodexOps, sources.mcpCodexOps, [
    'ToolDefinition::new',
    '"mission_codex_ops"',
    '"recent"',
    '"thread"',
    '"tool_stats"',
  ]);

  requireAll(diagnostics, files.mcpToolDirectory, sources.mcpToolDirectory, [
    'ToolDefinition::new',
    '"mission_tool_directory"',
    '"recommend"',
    '"lookup"',
    '"explain"',
    '"deprecated"',
  ]);

  requireAll(diagnostics, files.v2Source, sources.v2Source, [
    'capability-governance',
    'mission_capability_usage',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required text: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-capability-governance-'));
  for (const rel of Object.values(DEFAULT_FILES)) {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  }
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.blueprint),
    `
(missiond-blueprint
  (capability-governance-policy
    :review-sidecar ".missiond/v3/runtime/capability-usage-review.json"
    :protected-tool-patterns ["mission_execution" "mission_intent" "mission_forge_"]
    :protected-flow-patterns ["engineering" "F-execution-log-governance"])
  (mcp-tool-governance-policy
    :primary-families [mission_board mission_workflow mission_workstation mission_context mission_memory mission_universe mission_ops mission_router mission_tool_directory]
    :directory-tool mission_tool_directory)
  (v2-convergence-map
    (v2-item capability-governance :status runtime-projected))
  (public-surface-map
    (tool-group capability-audit-tools :status code-aligned)
    (tool-group mcp-tool-governance-tools :tools [mission_tool_directory]))
  (implementation-map
    (surface capability-governance
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/comm/capability_usage.rs"
             "crates/missiond-daemon/src/handlers/comm/capability_usage/runtime.rs"
             "crates/missiond-daemon/src/handlers/comm/audit.rs"
             "crates/missiond-daemon/src/handlers/comm/codex_ops.rs"
             "crates/missiond-daemon/src/handlers/comm/tool_directory.rs"
             "crates/missiond-mcp/src/tools/comm/capability_usage.rs"
             "crates/missiond-mcp/src/tools/comm/audit.rs"
             "crates/missiond-mcp/src/tools/comm/codex_ops.rs"
             "crates/missiond-mcp/src/tools/comm/tool_directory.rs"
             "scripts/check-v3-capability-governance-isomorphism.mjs"]
      :note "capability_usage.rs is the thin capability-governance facade; capability_usage/runtime.rs owns snapshot/report/candidates/mark/ack; audit.rs owns mission_audit trace/detail/stats/export; codex_ops.rs owns mission_codex_ops recent/thread/tool_stats; tool_directory.rs owns mission_tool_directory list/recommend/lookup/explain/deprecated."))
  (compression-contract
    :checks ["node scripts/check-v3-capability-governance-isomorphism.mjs"]))`,
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.dispatcher),
    '"mission_capability_usage" => capability_usage::handle "mission_tool_directory" => tool_directory::handle "mission_audit" => audit::handle "mission_codex_ops" => codex_ops::handle n.starts_with("mission_audit_") => audit::handle',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.capabilityFacade),
    'mod runtime; runtime::handle(state, name, args).await Thin V3 facade for the capability-governance read model',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.capabilityRuntime),
    'mission_capability_usage snapshot|report|candidates|mark|ack CapabilityUsageSnapshot CapabilityStaleCandidate CapabilityGovernanceRuntimeConfig load_governance_config V3_BLUEPRINT_CONFIG_ERROR LEGACY_CAPABILITY_REVIEW_SIDECAR review_sidecar_path protected_tool_patterns protected_flow_patterns action_snapshot action_report action_candidates action_mark action_ack source_coverage conversation_tool_calls board_tasks_flow_template event_log_flow_events lisp_semantic_hints review_sidecar workflow_execution_stats compute_protected_target_policy compute_review_required pick_merge_target apply_hint_overlay observability',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.v3Runtime),
    'CapabilityGovernanceRuntimeConfig parse_capability_governance_policy DEFAULT_CAPABILITY_REVIEW_SIDECAR DEFAULT_PROTECTED_TOOL_PATTERNS DEFAULT_PROTECTED_FLOW_PATTERNS capability-governance-policy :review-sidecar :protected-tool-patterns :protected-flow-patterns',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.audit),
    'mission_audit mission_audit_trace mission_audit_detail mission_audit_stats mission_audit_export format_tool_call_trace get_tool_calls_by_session get_tool_call_by_id get_board_task_notes',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.codexOps),
    'mission_codex_ops handle_recent handle_thread handle_tool_stats parse_since_to_utc codex_cli get_tool_calls_by_session get_tool_call_stats',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.toolDirectory),
    'mission_tool_directory ToolFamily FAMILIES recommend lookup_tool deprecated mission_board mission_workflow mission_workstation mission_context mission_memory mission_universe mission_ops mission_router',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.mcpCapability),
    'ToolDefinition::new "mission_capability_usage" "snapshot" "report" "candidates" "mark" "ack"',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.mcpAudit),
    'ToolDefinition::new "mission_audit" "trace" "detail" "stats" "export"',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.mcpCodexOps),
    'ToolDefinition::new "mission_codex_ops" "recent" "thread" "tool_stats"',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.mcpToolDirectory),
    'ToolDefinition::new "mission_tool_directory" "recommend" "lookup" "explain" "deprecated"',
  );
  fs.writeFileSync(
    path.join(root, DEFAULT_FILES.v2Source),
    'capability-governance mission_capability_usage',
  );
  return root;
}

main();
