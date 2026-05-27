#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-cascade-governance-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 cascade-governance Lisp/code isomorphism contract:
  - cascade.rs stays a thin route facade.
  - manifest/root path policy is isolated from graph/plan/trigger/lint behavior.
  - each public cascade MCP tool maps to a dedicated Rust module.
  - trigger execution projects V3 cascade-policy plus explicit env overrides.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  facade: 'crates/missiond-daemon/src/handlers/knowledge/cascade.rs',
  runtimeConfig: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  pathPolicy: 'crates/missiond-daemon/src/handlers/knowledge/cascade/path.rs',
  graph: 'crates/missiond-daemon/src/handlers/knowledge/cascade/graph.rs',
  plan: 'crates/missiond-daemon/src/handlers/knowledge/cascade/plan.rs',
  trigger: 'crates/missiond-daemon/src/handlers/knowledge/cascade/trigger.rs',
  lint: 'crates/missiond-daemon/src/handlers/knowledge/cascade/lint.rs',
  mcp: 'crates/missiond-mcp/src/tools/knowledge/cascade.rs',
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
    console.log('v3 cascade-governance Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 cascade-governance Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
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
    'cascade-governance',
    '(v2-item cascade-universe-governance',
    ':status runtime-projected',
    '(cascade-policy',
    ':default-manifest "$MISSIOND_PROJECTS_DIR/universe.intent.lisp"',
    ':allowed-root "$MISSIOND_PROJECTS_DIR"',
    ':trigger-enabled true',
    ':default-max-cycles 3',
    ':max-cycles-limit 12',
    '(surface cascade-governance',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
    'crates/missiond-daemon/src/handlers/knowledge/cascade.rs',
    'crates/missiond-daemon/src/handlers/knowledge/cascade/path.rs',
    'crates/missiond-daemon/src/handlers/knowledge/cascade/graph.rs',
    'crates/missiond-daemon/src/handlers/knowledge/cascade/plan.rs',
    'crates/missiond-daemon/src/handlers/knowledge/cascade/trigger.rs',
    'crates/missiond-daemon/src/handlers/knowledge/cascade/lint.rs',
    'crates/missiond-mcp/src/tools/knowledge/cascade.rs',
    'scripts/check-v3-cascade-governance-isomorphism.mjs',
    'cascade.rs is the thin cascade-governance facade',
    'cascade/path.rs owns manifest/root path policy by loading CascadeRuntimeConfig',
    'cascade/graph.rs owns mission_universe_graph',
    'cascade/plan.rs owns mission_cascade_plan dry-run',
    'cascade/trigger.rs owns mission_cascade_trigger',
    'V3 trigger-enabled plus CASCADE_TRIGGER_ENABLED explicit override',
    'TaskEvent::CascadeTriggered/Completed',
    'max-cycle clamp',
    'cascade/lint.rs owns mission_cascade_lint integrity egress',
    'node scripts/check-v3-cascade-governance-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.facade, sources.facade, [
    'mod graph;',
    'mod lint;',
    'mod path;',
    'mod plan;',
    'mod trigger;',
    '"mission_universe_graph" => graph::handle_universe_graph(args).await',
    '"mission_cascade_plan" => plan::handle_cascade_plan(args).await',
    '"mission_cascade_trigger" => trigger::handle_cascade_trigger(state, args).await',
    '"mission_cascade_lint" => lint::handle_cascade_lint(args).await',
    'unknown cascade tool',
  ]);

  requireAll(diagnostics, files.runtimeConfig, sources.runtimeConfig, [
    'CascadeRuntimeConfig',
    'DEFAULT_CASCADE_MANIFEST_PATH',
    'DEFAULT_CASCADE_ALLOWED_ROOT',
    'DEFAULT_CASCADE_TRIGGER_ENABLED',
    'DEFAULT_CASCADE_MAX_CYCLES',
    'MAX_CASCADE_MAX_CYCLES',
    'parse_cascade_policy',
    'load_for_current_dir',
    'nearest_missiond_root',
    'env_or_default_manifest_path',
    'env_or_allowed_root',
    'env_or_trigger_enabled',
    'clamp_max_cycles',
    'UNIVERSE_MANIFEST',
    'UNIVERSE_ROOT',
    'CASCADE_TRIGGER_ENABLED',
    'cascade-policy',
  ]);

  requireAll(diagnostics, files.pathPolicy, sources.pathPolicy, [
    'CascadeRuntimeConfig::load_for_current_dir',
    'resolve_manifest_path',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'env_or_default_manifest_path',
    'env_or_allowed_root',
    'canonicalize',
    'starts_with',
    'manifestPath',
  ]);

  requireAll(diagnostics, files.graph, sources.graph, [
    'UniverseGraphArgs',
    'handle_universe_graph',
    'default_format',
    'resolve_universe_graph',
    'format == "text"',
    '"service_count"',
    '"dependency_count"',
  ]);

  requireAll(diagnostics, files.plan, sources.plan, [
    'CascadePlanArgs',
    'handle_cascade_plan',
    'ServiceDelta',
    'CascadeConfig',
    'dry_run: true',
    'create_plan',
    '"upstream_map"',
  ]);

  requireAll(diagnostics, files.trigger, sources.trigger, [
    'CascadeTriggerArgs',
    'handle_cascade_trigger',
    'CascadeRuntimeConfig::load_for_current_dir',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'env_or_trigger_enabled',
    'clamp_max_cycles',
    'CASCADE_TRIGGER_ENABLED',
    'TaskEvent::CascadeTriggered',
    'TaskEvent::CascadeCompleted',
    'CascadeConfig',
    'max_repair_cycles',
    'dry_run: false',
    'tokio::task::spawn_blocking',
    'execute_plan',
    'hard_halted',
  ]);

  requireAll(diagnostics, files.lint, sources.lint, [
    'CascadeLintArgs',
    'handle_cascade_lint',
    'validate_universe_integrity',
    '"status": "clean"',
    '"error_count"',
    '"warning_count"',
    '"violations"',
  ]);

  requireAll(diagnostics, files.mcp, sources.mcp, [
    'ToolDefinition::new',
    '"mission_universe_graph"',
    '"mission_cascade_plan"',
    '"mission_cascade_trigger"',
    '"mission_cascade_lint"',
    'CASCADE_TRIGGER_ENABLED',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-cascade-governance-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (v2-convergence-map
    (v2-item cascade-universe-governance
      :status runtime-projected))
  (cascade-policy
    :default-manifest "$MISSIOND_PROJECTS_DIR/universe.intent.lisp"
    :allowed-root "$MISSIOND_PROJECTS_DIR"
    :trigger-enabled true
    :default-max-cycles 3
    :max-cycles-limit 12)
  (implementation-map
    (surface cascade-governance
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/path.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/graph.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/trigger.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/lint.rs"
             "crates/missiond-mcp/src/tools/knowledge/cascade.rs"
             "scripts/check-v3-cascade-governance-isomorphism.mjs"]
      :note "cascade.rs is the thin cascade-governance facade; cascade/path.rs owns manifest/root path policy by loading CascadeRuntimeConfig; cascade/graph.rs owns mission_universe_graph; cascade/plan.rs owns mission_cascade_plan dry-run; cascade/trigger.rs owns mission_cascade_trigger, V3 trigger-enabled plus CASCADE_TRIGGER_ENABLED explicit override, TaskEvent::CascadeTriggered/Completed, max-cycle clamp, and spawn_blocking execute_plan; cascade/lint.rs owns mission_cascade_lint integrity egress."))
  (compression-contract
    :checks ["node scripts/check-v3-cascade-governance-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.facade, `
mod graph;
mod lint;
mod path;
mod plan;
mod trigger;
"mission_universe_graph" => graph::handle_universe_graph(args).await
"mission_cascade_plan" => plan::handle_cascade_plan(args).await
"mission_cascade_trigger" => trigger::handle_cascade_trigger(state, args).await
"mission_cascade_lint" => lint::handle_cascade_lint(args).await
unknown cascade tool
`);

  writeFixture(root, DEFAULT_FILES.runtimeConfig, `
CascadeRuntimeConfig DEFAULT_CASCADE_MANIFEST_PATH DEFAULT_CASCADE_ALLOWED_ROOT
DEFAULT_CASCADE_TRIGGER_ENABLED DEFAULT_CASCADE_MAX_CYCLES MAX_CASCADE_MAX_CYCLES
parse_cascade_policy load_for_current_dir env_or_default_manifest_path
nearest_missiond_root env_or_allowed_root env_or_trigger_enabled clamp_max_cycles cascade-policy
UNIVERSE_MANIFEST UNIVERSE_ROOT CASCADE_TRIGGER_ENABLED
`);

  writeFixture(root, DEFAULT_FILES.pathPolicy, `
CascadeRuntimeConfig::load_for_current_dir resolve_manifest_path V3_BLUEPRINT_CONFIG_ERROR
env_or_default_manifest_path env_or_allowed_root
canonicalize starts_with manifestPath
`);

  writeFixture(root, DEFAULT_FILES.graph, `
UniverseGraphArgs handle_universe_graph default_format resolve_universe_graph
format == "text" "service_count" "dependency_count"
`);

  writeFixture(root, DEFAULT_FILES.plan, `
CascadePlanArgs handle_cascade_plan ServiceDelta CascadeConfig dry_run: true
create_plan "upstream_map"
`);

  writeFixture(root, DEFAULT_FILES.trigger, `
CascadeTriggerArgs handle_cascade_trigger CascadeRuntimeConfig::load_for_current_dir
V3_BLUEPRINT_CONFIG_ERROR env_or_trigger_enabled clamp_max_cycles CASCADE_TRIGGER_ENABLED
TaskEvent::CascadeTriggered TaskEvent::CascadeCompleted CascadeConfig
max_repair_cycles dry_run: false tokio::task::spawn_blocking execute_plan hard_halted
`);

  writeFixture(root, DEFAULT_FILES.lint, `
CascadeLintArgs handle_cascade_lint validate_universe_integrity
"status": "clean" "error_count" "warning_count" "violations"
`);

  writeFixture(root, DEFAULT_FILES.mcp, `
ToolDefinition::new
"mission_universe_graph" "mission_cascade_plan" "mission_cascade_trigger" "mission_cascade_lint"
CASCADE_TRIGGER_ENABLED
`);

  return root;
}

function writeFixture(root, rel, source) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, source);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
