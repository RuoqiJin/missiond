#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import {
  compiledFunctionMap,
  compiledSurfaceMap,
  compiledWorkflowMap,
  loadCompiledV3Contract,
} from './lib/v3_compiled_contract.mjs';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  workflow: '.missiond/workflows/nightly-evolution.lisp',
  analyzer: 'scripts/analyze-v3-self-evolution.mjs',
  runtime: 'crates/missiond-daemon/src/engine/nightly_evolution.rs',
  engineMod: 'crates/missiond-daemon/src/engine/mod.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  slotHandler: 'crates/missiond-daemon/src/handlers/compute/slot.rs',
  daemonHandlers: 'crates/missiond-daemon/src/handlers/mod.rs',
  masterControl: 'crates/missiond-daemon/src/engine/master_control.rs',
  mcpProcessTools: 'crates/missiond-mcp/src/tools/compute/process.rs',
  mcpGateway: 'crates/missiond-mcp/src/gen_gateway.rs',
  aggregate: 'scripts/check-v3-code-isomorphism-complete.mjs',
  gitignore: '.gitignore',
};

function main() {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(FILES)) {
    try {
      sources[key] = key === 'blueprint'
        ? readBlueprintWithEvidenceSidecars(process.cwd(), rel)
        : fs.readFileSync(path.join(process.cwd(), rel), 'utf8');
    } catch (err) {
      diagnostics.push(`${rel}: cannot read: ${err.message}`);
    }
  }
  if (diagnostics.length === 0) check(sources, diagnostics);
  if (diagnostics.length > 0) {
    diagnostics.forEach((d) => console.error(d));
    console.error(`v3 nightly-evolution check FAILED -- ${diagnostics.length} diagnostic(s)`);
    process.exit(1);
  }
  console.log('v3 nightly-evolution check OK');
}

function check(s, diagnostics) {
  const contract = loadCompiledV3Contract({ workflows: true });
  if (contract.ok !== true) {
    diagnostics.push(
      ...contract.diagnostics.map((d) => `${d.file}:${d.line}:${d.column}: ${d.message}`),
    );
  }
  const surfaces = compiledSurfaceMap(contract);
  const functions = compiledFunctionMap(contract);
  const workflows = compiledWorkflowMap(contract);
  if (!surfaces.has('nightly-evolution-loop')) {
    diagnostics.push('compiled semantic IR missing surface nightly-evolution-loop');
  }
  if (![...functions.values()].some((fn) => fn.surface === 'nightly-evolution-loop')) {
    diagnostics.push('compiled semantic IR missing function for surface nightly-evolution-loop');
  }
  if (!workflows.has('nightly-evolution')) {
    diagnostics.push('compiled workflows missing nightly-evolution');
  }
  requireAll(diagnostics, FILES.workflow, s.workflow, [
    '(workflow nightly-evolution',
    ':workflow_id nightly-evolution',
    ':status manual-first',
    'mission_nightly_evolution',
    'MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE',
    'Scheduled nightly evolution is disabled by default',
    'observe-only',
    'safe-backfill',
    'needs-investigation',
    'architecture-proposal',
    'requires-user-decision',
    'missiond-v3-blueprint',
    'compiled-semantic-ir',
    'compiled-workflows',
    'final-convergence-static-snapshot',
    'KB, historical conversations, provider logs, worker telemetry, Board open tasks',
    'recent commit history',
    'scripts/analyze-v3-self-evolution.mjs --json',
    'final-convergence-blocker',
    'facade-budget-near-limit',
    'oversized-authoring-block',
    'surface-flow-gap',
    '.missiond/v3/runtime/nightly-evolution/<date>.report.lisp',
    '.missiond/v3/runtime/self-evolution/<timestamp>-<finding_id>.proposal.lisp',
    ':proposal_id :finding_id :class :risk :summary :evidence_refs :affected_surfaces :recommended_change :acceptance :non_goals :created_at',
    'auto_execute=false',
    'must not auto-execute, hide, delete, or bulk-mutate historical tasks',
    'no KB task or memory mutation is created in default mode',
  ]);
  requireAll(diagnostics, FILES.blueprint, s.blueprint, [
    '(nightly-evolution-loop',
    '(function nightly-evolution',
    ':surface nightly-evolution-loop',
    ':policy (:workflow ".missiond/workflows/nightly-evolution.lisp"',
    ':schedule-window "manual-first"',
    ':schedule-enabled false',
    ':enable-env MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE',
    ':default-mode observe-only',
    ':proposal-artifact ".missiond/v3/runtime/self-evolution/<timestamp>-<finding_id>.proposal.lisp"',
    ':analyzer "scripts/analyze-v3-self-evolution.mjs --json"',
    ':risk-gate',
    'auto_execute=false',
    ':required [:proposal_id :finding_id :class :risk :summary :evidence_refs :affected_surfaces :recommended_change :acceptance :non_goals :created_at]',
    '(surface nightly-evolution-loop',
    'crates/missiond-daemon/src/engine/nightly_evolution.rs',
    'scripts/analyze-v3-self-evolution.mjs',
    'mission_nightly_evolution',
    'scripts/check-v3-nightly-evolution-isomorphism.mjs',
  ]);
  requireAll(diagnostics, FILES.analyzer, s.analyzer, [
    'scripts/analyze-v3-self-evolution.mjs',
    'COMPILED_SEMANTIC_IR',
    'COMPILED_WORKFLOWS',
    'check-v3-final-convergence.mjs',
    '--static-only',
    'final-convergence-blocker',
    'facade-budget-near-limit',
    'oversized-authoring-block',
    'surface-flow-gap',
    'SURFACE_FLOW_GAP_ALLOWLIST',
    'evidenceRefs',
    'affectedSurfaces',
    'recommendedChange',
    'acceptance',
    'nonGoals',
    '--dry-fixture',
  ]);
  requireAll(diagnostics, FILES.runtime, s.runtime, [
    'NIGHTLY_REPORT_DIR',
    'SELF_EVOLUTION_PROPOSAL_DIR',
    'SELF_EVOLUTION_ANALYZER',
    'MAX_SELF_EVOLUTION_PROPOSALS',
    'NightlyEvolutionRuntime',
    'start_nightly_evolution_service',
    'NIGHTLY_SCHEDULE_ENABLED_ENV',
    'nightly_evolution_schedule_enabled',
    'scheduleEnabled',
    'nightly-evolution schedule disabled by default',
    'MISSIOND_NIGHTLY_EVOLUTION_INTERVAL_SECS',
    'interval.tick().await;',
    'mission_nightly_evolution',
    'NightlyEvolutionArgs',
    'proposalPaths',
    'analyzerDiagnostics',
    'SelfEvolutionAnalyzerOutput',
    'SelfEvolutionAnalyzerRun',
    'ensure_compiled_runtime_available',
    'compile-v3-runtime.mjs',
    'run_self_evolution_analyzer',
    'analyzer_error_finding',
    'self-evolution-analyzer-error',
    'select_proposal_findings',
    'write_proposals',
    'render_proposal',
    ':proposal_id',
    ':finding_id',
    ':evidence_refs',
    ':affected_surfaces',
    ':recommended_change',
    ':non_goals',
    ':created_at',
    'auto_execute: Some(false)',
    'hidden: Some(false)',
    'read_final_convergence_snapshot',
    'check-v3-final-convergence.mjs',
    '--static-only',
    'create_requested_followup_if_needed',
    'build_followup_task_input',
    'UpsertTaskContractCommand',
    'nightly_followup_runtime_metadata',
    '"control_state": "task_contracts"',
    '"sandbox_profile": "system-self-evolution-review"',
    'proposal_selection_is_bounded_and_risk_sorted',
    'proposal_renderer_escapes_strings_and_uses_fixed_fields',
    'followup_task_is_visible_review_only',
    'status_snapshot',
    'nightly-evolution-report',
    ':proposal-paths',
    ':analyzer-diagnostics',
  ]);
  requireAll(diagnostics, FILES.engineMod, s.engineMod, ['pub mod nightly_evolution;']);
  requireAll(diagnostics, FILES.main, s.main, [
    'engine::nightly_evolution::start_nightly_evolution_service',
  ]);
  requireAll(diagnostics, FILES.slotHandler, s.slotHandler, [
    '"mission_nightly_evolution"',
    'nightly_evolution::mission_nightly_evolution(state, args).await',
  ]);
  requireAll(diagnostics, FILES.daemonHandlers, s.daemonHandlers, ['"mission_nightly_evolution"']);
  requireAll(diagnostics, FILES.masterControl, s.masterControl, [
    'nightlyEvolution',
    'crate::engine::nightly_evolution::status_snapshot().await',
  ]);
  requireAll(diagnostics, FILES.mcpProcessTools, s.mcpProcessTools, [
    '"mission_nightly_evolution"',
    'nightly evolution workflow',
    'observe-only',
  ]);
  requireAll(diagnostics, FILES.mcpGateway, s.mcpGateway, ['"mission_nightly_evolution"']);
  requireAll(diagnostics, FILES.aggregate, s.aggregate, [
    "'nightly-evolution-loop'",
    'scripts/check-v3-nightly-evolution-isomorphism.mjs',
  ]);
  requireAll(diagnostics, FILES.gitignore, s.gitignore, [
    '.missiond/v3/runtime/nightly-evolution/*.report.lisp',
    '.missiond/v3/runtime/self-evolution/*.proposal.lisp',
  ]);
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push(`${file}: missing required text: ${needle}`);
  }
}

main();
