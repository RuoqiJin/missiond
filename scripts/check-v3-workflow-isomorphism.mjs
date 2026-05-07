#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';
import { maybeRunLispc } from './lib/ocaml_lispc.mjs';

const usage = `Usage:
  node scripts/check-v3-workflow-isomorphism.mjs [--json] [--dry-fixture] [--engine=auto|js|ocaml]

Checks the V3 workflow Lisp/code isomorphism contract:
  - V3 blueprint declares workflow.lisp as the reusable workflow artifact.
  - mission_workflow distill emits workflow Lisp previews or Sonnet-distilled workflow_sexp.
  - compile_methodology reads workflow methodology Lisp and emits deterministic executable YAML.
  - distill file-first writes produce enriched V3 workflow artifacts.
  - review gates and auto-Sonnet policy are tied to the same surface.
  - MCP schema exposes the same distill/compile/run/review knobs.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  workflowHandler: 'crates/missiond-daemon/src/handlers/knowledge/workflow.rs',
  workflowArtifacts: 'crates/missiond-daemon/src/handlers/knowledge/workflow/artifacts.rs',
  workflowAutoChain: 'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain.rs',
  workflowAutoChainRecorder:
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain/recorder.rs',
  workflowAutoChainRules:
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain/rules.rs',
  workflowAutoSonnet: 'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet.rs',
  workflowAutoSonnetPolicy:
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet/policy.rs',
  workflowCompileMethodology:
    'crates/missiond-daemon/src/handlers/knowledge/workflow/compile_methodology.rs',
  workflowDistill: 'crates/missiond-daemon/src/handlers/knowledge/workflow/distill.rs',
  workflowMethodology: 'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs',
  workflowMethodologyExtract:
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/extract.rs',
  workflowMethodologyIo: 'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/io.rs',
  workflowMethodologySource:
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/source.rs',
  workflowMethodologyTypes:
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/types.rs',
  workflowMethodologyYaml:
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/yaml.rs',
  workflowProjectRoot: 'crates/missiond-daemon/src/handlers/knowledge/workflow/project_root.rs',
  workflowReviewResolution: 'crates/missiond-daemon/src/handlers/knowledge/workflow/review_resolution.rs',
  workflowRunMethodology:
    'crates/missiond-daemon/src/handlers/knowledge/workflow/run_methodology.rs',
  workflowStoreActions: 'crates/missiond-daemon/src/handlers/knowledge/workflow/store_actions.rs',
  workflowTests: 'crates/missiond-daemon/src/handlers/knowledge/workflow/tests.rs',
  mcpWorkflow: 'crates/missiond-mcp/src/tools/knowledge/workflow.rs',
  projectSsotConvergence: '.missiond/workflows/project-ssot-convergence.lisp',
  projectM6Depth: '.missiond/workflows/project-m6-depth.lisp',
  multiProjectM6Wave: '.missiond/workflows/multi-project-m6-wave.lisp',
};

function main() {
  const opts = parseArgs(process.argv.slice(2));

  const repoRoot = opts.dryFixture ? buildFixture() : process.cwd();
  const engineDiagnostics = [];
  const engine = runOcamlWorkflowChecks(repoRoot, opts.engine);
  if (engine.strictResult) {
    const result = {
      ok: engine.strictResult.ok === true,
      files: Object.keys(DEFAULT_FILES).length,
      engine,
      diagnostics: engine.strictResult.diagnostics ?? [],
    };
    writeResult(result, opts.json);
    process.exit(result.ok ? 0 : 1);
  }
  if (engine.mode === 'ocaml' && engine.ok === false) {
    engineDiagnostics.push(...(engine.diagnostics ?? []));
  }
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  diagnostics.push(...engineDiagnostics.map((d) => ({
    file: d.file ?? 'tools/missiond_lispc',
    message: `${d.code ?? 'OCAML_WORKFLOW'}: ${d.message}`,
  })));
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    engine,
    diagnostics,
  };

  writeResult(result, opts.json);

  process.exit(result.ok ? 0 : 1);
}

function parseArgs(args) {
  const opts = { json: false, dryFixture: false, engine: 'ocaml' };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--engine') {
      opts.engine = args[++i] ?? fail('--engine requires a value');
    } else if (arg.startsWith('--engine=')) {
      opts.engine = arg.slice('--engine='.length);
    } else {
      fail(`unknown arg: ${arg}`);
    }
  }
  if (!['auto', 'js', 'ocaml'].includes(opts.engine)) fail(`unknown engine: ${opts.engine}`);
  return opts;
}

function fail(message) {
  console.error(`${message}\n\n${usage}`);
  process.exit(2);
}

function runOcamlWorkflowChecks(repoRoot, engine) {
  if (engine === 'js') return { requested: engine, mode: 'js', ok: true, diagnostics: [] };
  const workflowDir = '.missiond/workflows';
  const attempt = maybeRunLispc(['check-workflow-dir', '--workflow-dir', workflowDir], { engine, repoRoot });
  if (attempt.mode === 'js-fallback') {
    return {
      requested: engine,
      mode: 'js-fallback',
      ok: true,
      diagnostics: attempt.result?.diagnostics ?? [],
    };
  }
  const result = attempt.result;
  if (engine === 'ocaml' && result?.unavailable) {
    return { requested: engine, mode: 'ocaml', strictResult: result };
  }
  const diagnostics = result?.ok === true ? [] : (result?.diagnostics ?? []);
  return { requested: engine, mode: 'ocaml', ok: diagnostics.length === 0, diagnostics };
}

function writeResult(result, json) {
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 workflow Lisp/code isomorphism check OK');
  } else {
    for (const d of result.diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(`v3 workflow Lisp/code isomorphism check FAILED -- ${result.diagnostics.length} diagnostic(s)`);
  }
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
    '(artifact workflow',
    ':path ".missiond/workflows/<topic>.lisp"',
    ':writer workflow-distiller',
    ':required [:workflow_id :source_plans :match_rules :steps :status]',
    '(surface mission_workflow',
    ':status "code-aligned"',
    'distill dry_run emits workflow-draft Lisp',
    'sonnet distiller requires JSON workflow_sexp + object match_rules',
    'mission_workflow sonnet distiller compiler_model labels',
    'router-runtime-policy queued_sonnet_model',
    'distill persist+write_file writes an enriched V3 workflow artifact',
    ':body workflow_sexp',
    'compile_methodology reads methodology Lisp from .missiond/workflows/<name>.lisp',
    'persist+write_file path now also projects the methodology compile through render_workflow_artifact_sexp',
    'source_kind=methodology',
    'artifact_only_no_workflow_row',
    'workflow_record_execution(success=true,cost_usd?)',
    ':status compiled',
    'no Workflow DB row',
    'instead of canonicalizing the raw methodology source',
    'ArtifactKind::Workflow',
    'auto_sonnet_policy={off|safe_after_rules|dry_run}',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/artifacts.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain/recorder.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain/rules.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet/policy.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/compile_methodology.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/distill.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/extract.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/io.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/source.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/types.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/yaml.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/project_root.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/review_resolution.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/run_methodology.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/store_actions.rs',
    'workflow/distill.rs owns DistillMode',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/tests.rs',
    'workflow/review_resolution.rs owns resolve_review',
    'WorkflowSubscriberOutcome',
    'node scripts/check-v3-workflow-isomorphism.mjs',
    '(agent-interaction-policy',
    ':schema "missiond.agent-interaction-policy.v1"',
    '(role resident-master',
    '(role investigator-worker',
    '(role implementer-worker',
    '(role deterministic-llm-tool',
    ':required-output-fields [decision reasoning_summary evidence_needed delegation_plan? next_question_or_action]',
    ':forbidden-default-inputs [kb board-backlog historical-conversation provider-durable-logs]',
    'exact-shard-ready=true',
    'questions, hypotheses, evidence_needed, findings, design_options, and accepted_shards',
  ]);

  requireAll(diagnostics, files.projectSsotConvergence, sources.projectSsotConvergence, [
    ':inputs [project-id project-root canonical-intent existing-code dirty-baseline context-pack-path? acceptance]',
    ':id choose-write-strategy',
    'prefer overlay+manifest mode',
    'if present and large, add an M6 overlay rather than replacing the whole file',
    'run scoped diff checks for owned paths',
    ':id m6-depth-handoff',
    '.missiond/workflows/project-m6-depth.lisp',
    'M6 production-readiness claims require domain model audit',
    ':id worker-stall-recovery',
    "stalls after intermediate narration such as 'let me write'",
    'reduce the shard to an atomic overlay/manifest patch',
    'Dirty worktree SSOT convergence commits must stage explicit .missiond paths only',
    'Large existing intent files should use M6 overlay+manifest',
    'Dirty-baseline handling is explicit',
  ]);

  requireAll(diagnostics, files.projectM6Depth, sources.projectM6Depth, [
    ':workflow_id project-m6-depth',
    ':status active',
    ':source_plans [auth-m6-depth project-ssot-convergence v3-runtime-ssot]',
    ':id review-question',
    ':id evidence-plan',
    ':id investigation',
    ':id synthesis',
    ':id design-proposal',
    ':id domain-model-audit',
    ':id target-architecture-draft',
    ':id authority-chain-check',
    ':id compatibility-ledger',
    ':id runtime-registration-check',
    ':id event-contract-check',
    ':id hot-path-wiring-check',
    ':id regression-matrix',
    ':id exact-shards',
    ':id implementation',
    ':id verification',
    ':context-pack-artifacts [questions hypotheses evidence_needed findings design_options accepted_shards]',
    'exact-shard-ready=true',
    'tenant -> application -> product -> product_user -> product_user_group',
    'Critical contracts must be hot-path wired',
    'No destructive DB migration',
    'No production deploy, DNS mutation, or secret mutation',
    'Runtime registration of new business objects does not require rebuild or redeploy',
  ]);

  requireAll(diagnostics, files.multiProjectM6Wave, sources.multiProjectM6Wave, [
    ':workflow_id multi-project-m6-wave',
    ':status active',
    ':id select-wave',
    ':id review-question',
    ':id evidence-plan',
    ':id investigation',
    ':id synthesis',
    ':id design-proposal',
    ':id exact-shards',
    ':id implementation',
    ':id verification',
    ':context-pack-artifacts [questions hypotheses evidence_needed findings design_options accepted_shards]',
    'Findings / Evidence / Recommendations / Verification',
    'exact-shard-ready=true',
    'check-project-maturity --min-level M6',
  ]);

  const workflowSurface = `${sources.workflowHandler}\n${sources.workflowArtifacts}\n${sources.workflowAutoChain}\n${sources.workflowAutoChainRecorder}\n${sources.workflowAutoChainRules}\n${sources.workflowAutoSonnet}\n${sources.workflowAutoSonnetPolicy}\n${sources.workflowCompileMethodology}\n${sources.workflowDistill}\n${sources.workflowMethodology}\n${sources.workflowMethodologyExtract}\n${sources.workflowMethodologyIo}\n${sources.workflowMethodologySource}\n${sources.workflowMethodologyTypes}\n${sources.workflowMethodologyYaml}\n${sources.workflowProjectRoot}\n${sources.workflowReviewResolution}\n${sources.workflowRunMethodology}\n${sources.workflowStoreActions}\n${sources.workflowTests}`;
  const workflowSurfaceLabel = `${files.workflowHandler} + ${files.workflowArtifacts} + ${files.workflowAutoChain} + ${files.workflowAutoChainRecorder} + ${files.workflowAutoChainRules} + ${files.workflowAutoSonnet} + ${files.workflowAutoSonnetPolicy} + ${files.workflowCompileMethodology} + ${files.workflowDistill} + ${files.workflowMethodology} + ${files.workflowMethodologyExtract} + ${files.workflowMethodologyIo} + ${files.workflowMethodologySource} + ${files.workflowMethodologyTypes} + ${files.workflowMethodologyYaml} + ${files.workflowProjectRoot} + ${files.workflowReviewResolution} + ${files.workflowRunMethodology} + ${files.workflowStoreActions} + ${files.workflowTests}`;
  requireAll(diagnostics, workflowSurfaceLabel, workflowSurface, [
    'enum DistillMode',
    'fn parse_distill_mode',
    'Some("dry_run") => Ok(DistillMode::DryRun)',
    'Some("sonnet") => Ok(DistillMode::Sonnet)',
    'async fn action_distill_dry_run',
    '"(workflow-draft\\n  :name',
    '"compiled_sexp_preview": preview_sexp',
    '.workflow_insert(name, &preview_sexp, &json!({}), Some(plan.id))',
    'render_workflow_artifact_sexp(',
    'async fn action_distill_sonnet',
    'build_distiller_prompt(plan, name, &match_hint, &evidence_value)',
    '"workflow_sexp"',
    'validate_workflow_sexp(&workflow_sexp)',
    '"match_rules"',
    'match_rules.is_object()',
    '.workflow_insert(name, &workflow_sexp, &match_rules, Some(plan.id))',
    'fn validate_workflow_sexp',
    'paren_balanced_ignoring_strings(trimmed)',
    'enum CompileMode',
    'fn parse_compile_mode',
    'async fn action_compile_methodology',
    'fn action_compile_dry_run',
    'async fn action_compile_deterministic',
    'validate_methodology_source(content)',
    'extract_steps_with_lines(content)',
    'async fn action_run_methodology',
    'parse_run_methodology_record_intent',
    'methodology_execution_record_payload',
    'fn extract_workflow_file_args',
    'ArtifactKind::Workflow',
    'attempt_artifact_write(',
    'fn render_workflow_artifact_sexp',
    ':workflow_id',
    ':source_plans',
    ':match_rules',
    ':steps',
    ':body',
    'fn build_methodology_match_rules',
    '"source_kind": "methodology"',
    '"compiler": "deterministic-v0"',
    '"compiler_version": COMPILER_VERSION',
    'methodology_match_rules = build_methodology_match_rules(&meta)',
    'render_workflow_artifact_sexp(',
    'methodology_compile_renders_v3_workflow_artifact_not_raw_source',
    'methodology_compile_review_required_status_when_no_steps',
    'build_methodology_match_rules_includes_flow_id_and_source_hash',
    'fn json_to_lisp',
    'fn render_workflow_steps',
    'parse_review_gate_policy(args)',
    'apply_compile_review_gates(',
    'fn parse_auto_sonnet_policy',
    '"safe_after_rules"',
    'mod tests;',
  ]);

  requireAll(diagnostics, files.workflowHandler, sources.workflowHandler, [
    'mod compile_methodology;',
    'mod distill;',
    'mod project_root;',
    'mod run_methodology;',
    'mod store_actions;',
    'load_compiled_workflow_contracts',
    'use compile_methodology::action_compile_methodology;',
    'use distill::action_distill;',
    'use run_methodology::action_run_methodology;',
  ]);

  requireAll(diagnostics, files.workflowStoreActions, sources.workflowStoreActions, [
    'pub(super) async fn action_list',
    'compiled_workflow_contracts_for_args',
    '"compiledContracts"',
    '"source": "compiled-workflows"',
    'resolve_project_root_from_args(state, args)',
    'pub(super) async fn action_get',
    'pub(super) async fn action_match',
    'pub(super) async fn action_apply',
    'pub(super) async fn action_record_execution',
    'pub(super) fn parse_id_arg',
    'workflow_record_execution',
  ]);

  requireAll(diagnostics, files.workflowCompileMethodology, sources.workflowCompileMethodology, [
    'pub(super) enum CompileMode',
    'pub(super) fn parse_compile_mode',
    'pub(super) async fn action_compile_methodology',
    'pub(super) fn action_compile_dry_run',
    'pub(super) async fn action_compile_deterministic',
    'validate_methodology_source(content)',
    'extract_steps_with_lines(content)',
    'methodology_match_rules = build_methodology_match_rules(&meta)',
    'pub(super) fn count_top_form',
  ]);

  requireAll(diagnostics, files.workflowRunMethodology, sources.workflowRunMethodology, [
    'pub(super) async fn action_run_methodology',
    'parse_run_methodology_record_intent',
    'methodology_execution_record_payload',
    'resolve_compiled_flow',
    'MISSING_COMPILED_FLOW',
    'CreateBoardTaskInput',
    'runner::run_flow',
    'workflow_record_execution',
    'artifact_only_no_workflow_row',
  ]);

  requireAll(diagnostics, files.workflowProjectRoot, sources.workflowProjectRoot, [
    'pub(super) async fn resolve_project_root_from_args',
    'pub(super) async fn resolve_project_root_with_registry',
    'resolve_target_project_root',
    'refuses process-cwd fallback',
  ]);

  requireAll(diagnostics, files.workflowDistill, sources.workflowDistill, [
    'RouterRuntimeConfig',
    'fn load_sonnet_compiler_model',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'queued_sonnet_model',
    'pub(super) enum DistillMode',
    'pub(super) fn parse_distill_mode',
    'pub(super) async fn action_distill',
    'async fn action_distill_dry_run',
    'pub(super) async fn action_distill_sonnet',
    'build_distiller_prompt(plan, name, &match_hint, &evidence_value)',
    'load_sonnet_compiler_model()',
    '"compiler_model": compiler_model',
    'pub(super) enum EvidenceOutcome',
    'pub(super) fn evidence_sidecar_path',
    'pub(super) fn read_evidence_sidecar',
    'pub(super) fn validate_workflow_sexp',
    'pub(super) fn paren_balanced_ignoring_strings',
  ]);
  forbidAll(diagnostics, files.workflowDistill, sources.workflowDistill, [
    'const SONNET_COMPILER_MODEL',
    'SONNET_COMPILER_MODEL: &str = "claude-sonnet"',
  ]);

  requireAll(diagnostics, files.workflowMethodology, sources.workflowMethodology, [
    'mod extract;',
    'mod io;',
    'mod source;',
    'mod types;',
    'mod yaml;',
    'pub(in crate::handlers::knowledge::workflow) use self::extract::*;',
    'pub(in crate::handlers::knowledge::workflow) use self::io::*;',
    'pub(in crate::handlers::knowledge::workflow) use self::source::*;',
    'pub(in crate::handlers::knowledge::workflow) use self::types::*;',
    'pub(in crate::handlers::knowledge::workflow) use self::yaml::*;',
  ]);

  requireAll(diagnostics, files.workflowMethodologyTypes, sources.workflowMethodologyTypes, [
    'pub(in crate::handlers::knowledge::workflow) struct MethodologyStep',
    'pub(in crate::handlers::knowledge::workflow) struct MethodologyForm',
    'pub(in crate::handlers::knowledge::workflow) struct MethodologyLifted',
    'pub(in crate::handlers::knowledge::workflow) struct GeneratedMeta',
    'pub(in crate::handlers::knowledge::workflow) enum CompiledFlowError',
  ]);

  requireAll(diagnostics, files.workflowMethodologySource, sources.workflowMethodologySource, [
    'pub(in crate::handlers::knowledge::workflow) fn resolve_methodology_path',
    'pub(in crate::handlers::knowledge::workflow) fn validate_methodology_source',
    'pub(in crate::handlers::knowledge::workflow) fn source_hash',
    'pub(in crate::handlers::knowledge::workflow) fn derive_flow_id',
    'pub(in crate::handlers::knowledge::workflow) fn resolve_compiled_flow',
  ]);

  requireAll(diagnostics, files.workflowMethodologyExtract, sources.workflowMethodologyExtract, [
    'pub(in crate::handlers::knowledge::workflow) fn extract_steps',
    'pub(in crate::handlers::knowledge::workflow) fn extract_steps_with_lines',
    'pub(in crate::handlers::knowledge::workflow) fn extract_methodology_lifted',
    'pub(in crate::handlers::knowledge::workflow) fn match_form_keyword',
    'pub(in crate::handlers::knowledge::workflow) fn parse_optional_form_id',
    'pub(in crate::handlers::knowledge::workflow) fn phase_id_for_step',
  ]);

  requireAll(diagnostics, files.workflowMethodologyYaml, sources.workflowMethodologyYaml, [
    'pub(in crate::handlers::knowledge::workflow) fn build_generated_yaml',
    'pub(in crate::handlers::knowledge::workflow) fn build_manual_review_prompt',
    'fn build_methodology_metadata_yaml',
  ]);

  requireAll(diagnostics, files.workflowMethodologyIo, sources.workflowMethodologyIo, [
    'pub(in crate::handlers::knowledge::workflow) fn unique_generated_yaml_temp_path',
    'pub(in crate::handlers::knowledge::workflow) fn atomic_write',
    'GENERATED_YAML_TEMP_SEQ',
  ]);

  requireAll(diagnostics, files.workflowReviewResolution, sources.workflowReviewResolution, [
    'pub(super) async fn action_resolve_review',
    'pub(super) const WORKFLOW_REVIEW_ACTIONS',
    'pub(super) const WORKFLOW_REVIEW_VERSION',
    'pub(crate) enum WorkflowSubscriberOutcome',
    'pub(crate) async fn handle_review_resolved_event',
    'ResolutionOutcome::RequestChanges',
    'WorkflowSubscriberOutcome::MethodologyReceipt',
  ]);

  requireAll(diagnostics, files.workflowArtifacts, sources.workflowArtifacts, [
    'pub(super) fn extract_workflow_file_args',
    'pub(super) async fn maybe_write_workflow_artifact',
    'pub(super) fn render_workflow_artifact_sexp',
    'pub(super) fn build_methodology_match_rules',
  ]);

  requireAll(diagnostics, files.workflowAutoChain, sources.workflowAutoChain, [
    'mod recorder;',
    'mod rules;',
    'pub(super) use recorder::{',
    'pub(super) use rules::{',
    'pub(super) async fn maybe_apply_distill_chain_layers',
    'pub(super) enum AutoChainTrigger',
    'pub(super) fn build_auto_trigger_block',
    'pub(super) fn attach_auto_trigger_to_payload',
  ]);

  requireAll(diagnostics, files.workflowAutoChainRecorder, sources.workflowAutoChainRecorder, [
    'pub(in crate::handlers::knowledge::workflow) async fn maybe_apply_auto_chain',
    'pub(in crate::handlers::knowledge::workflow) fn auto_chain_requested',
    'pub(in crate::handlers::knowledge::workflow) fn derive_auto_chain_id',
    'pub(in crate::handlers::knowledge::workflow) fn compute_evidence_sha256',
    'pub(in crate::handlers::knowledge::workflow) fn build_auto_chain_block',
    'pub(in crate::handlers::knowledge::workflow) fn attach_auto_chain_to_payload',
    'AUTO_CHAIN_EVIDENCE_SOURCE',
    'evidence_collector::append',
    'workflow_distill_auto_chain',
  ]);

  requireAll(diagnostics, files.workflowAutoChainRules, sources.workflowAutoChainRules, [
    'pub(in crate::handlers::knowledge::workflow) fn parse_auto_chain_trigger',
    'pub(in crate::handlers::knowledge::workflow) fn evaluate_auto_trigger_safety_rules',
    'pub(in crate::handlers::knowledge::workflow) fn render_safety_rule_results',
    'pub(in crate::handlers::knowledge::workflow) fn inner_result_is_error',
    'pub(in crate::handlers::knowledge::workflow) fn chain_id_already_in_sidecar',
    'pub(in crate::handlers::knowledge::workflow) struct SafetyRuleContext',
    'AUTO_TRIGGER_DEFAULT_MIN_EVIDENCE',
    'SAFETY_RULE_INNER_DISTILL_OK',
    'EvidenceOutcome::Present',
  ]);

  requireAll(diagnostics, files.workflowAutoSonnet, sources.workflowAutoSonnet, [
    'mod policy;',
    'pub(super) use policy::*;',
    'pub(super) fn validate_auto_sonnet_args',
    'pub(super) fn auto_sonnet_requested',
    'pub(super) async fn maybe_apply_auto_sonnet',
    'pub(super) async fn maybe_apply_auto_sonnet_no_trigger',
  ]);

  requireAll(diagnostics, files.workflowAutoSonnetPolicy, sources.workflowAutoSonnetPolicy, [
    'use super::*;',
    'pub(in crate::handlers::knowledge::workflow) enum AutoSonnetPolicy',
    'pub(in crate::handlers::knowledge::workflow) fn parse_auto_sonnet_policy',
    'pub(in crate::handlers::knowledge::workflow) async fn maybe_apply_auto_sonnet_policy',
    'AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR',
    'review_required=true',
  ]);

  requireAll(diagnostics, files.workflowTests, sources.workflowTests, [
    'use super::*;',
    'methodology_compile_renders_v3_workflow_artifact_not_raw_source',
    'methodology_compile_review_required_status_when_no_steps',
    'build_methodology_match_rules_includes_flow_id_and_source_hash',
    'run_methodology_record_intent_defaults_to_artifact_only',
    'run_methodology_record_intent_accepts_workflow_row_target',
  ]);

  requireAll(diagnostics, files.mcpWorkflow, sources.mcpWorkflow, [
    'manager action — see Lisp implemented-surface mission_workflow',
    '"distill"',
    '"compile_methodology"',
    '"run_methodology"',
    '"resolve_review"',
    'artifact_only_no_workflow_row',
    'workflow_record_execution(success=true,cost_usd?)',
    '"distill_mode"',
    '&["dry_run", "sonnet"]',
    '"compile_mode"',
    '&["dry_run", "deterministic"]',
    '"write_file"',
    'enriched V3 workflow artifact carrying :workflow_id/:source_plans/:match_rules/:steps/:status plus :body workflow_sexp',
    'compile_methodology has no Workflow DB row',
    'deterministic generated flow_id',
    'source_kind=\\"methodology\\"',
    '"overwrite_file"',
    '.missiond/v3/runtime/plans/<plan_id>.evidence.json',
    '"review_gate_policy"',
    '"review_automation_policy"',
    '"auto_sonnet_policy"',
    '&["off", "safe_after_rules", "dry_run"]',
    'Lisp 源: intent-flow.lisp',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    requireText(diagnostics, file, source, needle);
  }
}

function forbidAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push({ file, message: `forbidden contract text present: ${needle}` });
    }
  }
}

function requireText(diagnostics, file, source, needle) {
  if (!source.includes(needle)) {
    diagnostics.push({ file, message: `missing required contract text: ${needle}` });
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-workflow-isomorphism-'));
  fs.mkdirSync(path.join(root, 'tools'), { recursive: true });
  fs.symlinkSync(path.resolve('tools/missiond_lispc'), path.join(root, 'tools/missiond_lispc'), 'dir');
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (artifact-contracts
    (artifact workflow
      :path ".missiond/workflows/<topic>.lisp"
      :writer workflow-distiller
      :required [:workflow_id :source_plans :match_rules :steps :status]))
  (implementation-map
    (surface mission_workflow
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/knowledge/workflow/artifacts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain/recorder.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain/rules.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet/policy.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/compile_methodology.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/distill.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/extract.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/io.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/source.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/types.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/yaml.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/project_root.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/review_resolution.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/run_methodology.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/store_actions.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/tests.rs"]
      :model-projection "mission_workflow sonnet distiller compiler_model labels project from router-runtime-policy queued_sonnet_model"
      :note "workflow/distill.rs owns DistillMode and action_distill. distill dry_run emits workflow-draft Lisp; sonnet distiller requires JSON workflow_sexp + object match_rules; distill persist+write_file writes an enriched V3 workflow artifact with :body workflow_sexp; workflow/run_methodology.rs owns parse_run_methodology_record_intent and methodology_execution_record_payload; workflow/methodology.rs is the compile_methodology facade; methodology/types.rs owns methodology compiler data shapes; methodology/source.rs owns methodology path/source/hash/flow resolution; methodology/extract.rs owns step and higher-order form lifting; methodology/yaml.rs owns generated executable YAML projection; methodology/io.rs owns unique temp path and atomic write. compile_methodology reads methodology Lisp from .missiond/workflows/<name>.lisp; persist+write_file path now also projects the methodology compile through render_workflow_artifact_sexp with :match_rules carrying source_kind=methodology / compiler / compiler_version / source_hash / flow_id, :status compiled, :body methodology lisp body, instead of canonicalizing the raw methodology source — no Workflow DB row is introduced; ArtifactKind::Workflow; run_methodology returns artifact_only_no_workflow_row unless caller supplies workflow_id, then records workflow_record_execution(success=true,cost_usd?); workflow/auto_chain/recorder.rs owns the wave-19 explicit recorder; workflow/auto_chain/rules.rs owns deterministic auto-trigger rule evaluation; workflow/auto_sonnet.rs owns the wave-21 dual opt-in gate; workflow/auto_sonnet/policy.rs owns auto_sonnet_policy={off|safe_after_rules|dry_run}; workflow/review_resolution.rs owns resolve_review and WorkflowSubscriberOutcome"))
  (agent-interaction-policy
    :schema "missiond.agent-interaction-policy.v1"
    (role resident-master
      :required-output-fields [decision reasoning_summary evidence_needed delegation_plan? next_question_or_action]
      :forbidden-default-inputs [kb board-backlog historical-conversation provider-durable-logs]
      :rule "exact-shard-ready=true")
    (role investigator-worker :rule "Findings / Evidence / Recommendations / Verification")
    (role implementer-worker :rule "accepted shard")
    (role deterministic-llm-tool :rule "precise prompts")
    :runtime-invariants ["questions, hypotheses, evidence_needed, findings, design_options, and accepted_shards"])
  (compression-contract
    :checks ["node scripts/check-v3-workflow-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.workflowHandler, `
mod compile_methodology;
mod distill;
mod project_root;
mod run_methodology;
mod store_actions;
use crate::context::v3_blueprint_runtime::load_compiled_workflow_contracts;
use compile_methodology::action_compile_methodology;
use distill::action_distill;
use run_methodology::action_run_methodology;
#[cfg(test)]
mod tests;
`);
  writeFixture(root, DEFAULT_FILES.workflowStoreActions, `
pub(super) async fn action_list() { "compiledContracts"; }
fn compiled_workflow_contracts_for_args() {
  resolve_project_root_from_args(state, args);
  "source": "compiled-workflows";
}
pub(super) async fn action_get() {}
pub(super) async fn action_match() {}
pub(super) async fn action_apply() {}
pub(super) async fn action_record_execution() { workflow_record_execution(); }
pub(super) fn parse_id_arg() {}
`);
  writeFixture(root, DEFAULT_FILES.workflowCompileMethodology, `
enum DistillMode {}
fn parse_distill_mode() {
  Some("dry_run") => Ok(DistillMode::DryRun);
  Some("sonnet") => Ok(DistillMode::Sonnet);
}
async fn action_distill_dry_run() {
  "(workflow-draft\\n  :name";
  "compiled_sexp_preview": preview_sexp;
  .workflow_insert(name, &preview_sexp, &json!({}), Some(plan.id));
  render_workflow_artifact_sexp();
}
async fn action_distill_sonnet() {
  build_distiller_prompt(plan, name, &match_hint, &evidence_value);
  "workflow_sexp";
  validate_workflow_sexp(&workflow_sexp);
  "match_rules";
  match_rules.is_object();
  .workflow_insert(name, &workflow_sexp, &match_rules, Some(plan.id));
}
fn validate_workflow_sexp() { paren_balanced_ignoring_strings(trimmed); }
pub(super) enum CompileMode {}
pub(super) fn parse_compile_mode() {}
pub(super) async fn action_compile_methodology() {}
pub(super) fn action_compile_dry_run() {}
pub(super) async fn action_compile_deterministic() {
  validate_methodology_source(content);
  extract_steps_with_lines(content);
  let methodology_match_rules = build_methodology_match_rules(&meta);
  render_workflow_artifact_sexp(&meta.flow_id, &[], &methodology_match_rules, "compiled", content);
  parse_review_gate_policy(args);
  apply_compile_review_gates();
}
pub(super) fn count_top_form() {}
`);
  writeFixture(root, DEFAULT_FILES.workflowRunMethodology, `
pub(super) fn parse_run_methodology_record_intent() {}
pub(super) fn methodology_execution_record_payload() { "artifact_only_no_workflow_row"; }
pub(super) async fn action_run_methodology() {
  resolve_compiled_flow();
  "MISSING_COMPILED_FLOW";
  CreateBoardTaskInput;
  runner::run_flow();
  workflow_record_execution();
}
`);
  writeFixture(root, DEFAULT_FILES.workflowProjectRoot, `
pub(super) async fn resolve_project_root_from_args() {}
pub(super) async fn resolve_project_root_with_registry() {
  resolve_target_project_root();
  "refuses process-cwd fallback";
}
`);
  writeFixture(root, DEFAULT_FILES.workflowArtifacts, `
fn extract_workflow_file_args() {}
ArtifactKind::Workflow;
attempt_artifact_write();
fn render_workflow_artifact_sexp() { ":workflow_id"; ":source_plans"; ":match_rules"; ":steps"; ":body"; }
fn build_methodology_match_rules() {
  "source_kind": "methodology";
  "compiler": "deterministic-v0";
  "compiler_version": COMPILER_VERSION;
}
fn json_to_lisp() {}
fn render_workflow_steps() {}
parse_review_gate_policy(args);
apply_compile_review_gates();
fn parse_auto_sonnet_policy() { "safe_after_rules"; }
#[cfg(test)]
mod tests;
`);
  writeFixture(root, DEFAULT_FILES.workflowDistill, `
use crate::context::v3_blueprint_runtime::RouterRuntimeConfig;
fn load_sonnet_compiler_model() {
  let router_config = RouterRuntimeConfig::load_for_current_dir().unwrap();
  let _ = "V3_BLUEPRINT_CONFIG_ERROR";
  let _ = router_config.queued_sonnet_model;
}
pub(super) enum DistillMode {}
pub(super) fn parse_distill_mode() {
  Some("dry_run") => Ok(DistillMode::DryRun);
  Some("sonnet") => Ok(DistillMode::Sonnet);
}
pub(super) async fn action_distill() {}
async fn action_distill_dry_run() {
  "(workflow-draft\\n  :name";
  "compiled_sexp_preview": preview_sexp;
  .workflow_insert(name, &preview_sexp, &json!({}), Some(plan.id));
  render_workflow_artifact_sexp();
}
pub(super) async fn action_distill_sonnet() {
  build_distiller_prompt(plan, name, &match_hint, &evidence_value);
  let compiler_model = load_sonnet_compiler_model().unwrap();
  "compiler_model": compiler_model;
  "workflow_sexp";
  validate_workflow_sexp(&workflow_sexp);
  "match_rules";
  match_rules.is_object();
  .workflow_insert(name, &workflow_sexp, &match_rules, Some(plan.id));
}
pub(super) enum EvidenceOutcome {}
pub(super) fn evidence_sidecar_path() {}
pub(super) fn read_evidence_sidecar() {}
pub(super) fn validate_workflow_sexp() { paren_balanced_ignoring_strings(trimmed); }
pub(super) fn paren_balanced_ignoring_strings() {}
`);
  writeFixture(root, DEFAULT_FILES.workflowTests, `
use super::*;
fn methodology_compile_renders_v3_workflow_artifact_not_raw_source() {}
fn methodology_compile_review_required_status_when_no_steps() {}
fn build_methodology_match_rules_includes_flow_id_and_source_hash() {}
fn run_methodology_record_intent_defaults_to_artifact_only() {}
fn run_methodology_record_intent_accepts_workflow_row_target() {}`);
  writeFixture(root, DEFAULT_FILES.workflowArtifacts, `
pub(super) fn extract_workflow_file_args() {}
pub(super) async fn maybe_write_workflow_artifact() {}
ArtifactKind::Workflow;
attempt_artifact_write();
pub(super) fn render_workflow_artifact_sexp() { ":workflow_id"; ":source_plans"; ":match_rules"; ":steps"; ":body"; }
pub(super) fn build_methodology_match_rules() {
  "source_kind": "methodology";
  "compiler": "deterministic-v0";
  "compiler_version": COMPILER_VERSION;
}
fn json_to_lisp() {}
fn render_workflow_steps() {}`);
  writeFixture(root, DEFAULT_FILES.workflowAutoChain, `
mod recorder;
mod rules;
pub(super) use recorder::{maybe_apply_auto_chain, AUTO_CHAIN_EVIDENCE_SOURCE};
pub(super) use rules::{evaluate_auto_trigger_safety_rules, parse_auto_chain_trigger};
pub(super) async fn maybe_apply_distill_chain_layers() {}
pub(super) enum AutoChainTrigger {}
pub(super) fn build_auto_trigger_block() {}
pub(super) fn attach_auto_trigger_to_payload() {}
`);
  writeFixture(root, DEFAULT_FILES.workflowAutoChainRecorder, `
pub(in crate::handlers::knowledge::workflow) async fn maybe_apply_auto_chain() {}
pub(in crate::handlers::knowledge::workflow) fn auto_chain_requested() {}
pub(in crate::handlers::knowledge::workflow) fn derive_auto_chain_id() {}
pub(in crate::handlers::knowledge::workflow) fn compute_evidence_sha256() {}
pub(in crate::handlers::knowledge::workflow) fn build_auto_chain_block() {}
pub(in crate::handlers::knowledge::workflow) fn attach_auto_chain_to_payload() {}
const AUTO_CHAIN_EVIDENCE_SOURCE: &str = "workflow_distill_auto_chain";
evidence_collector::append();
// workflow_distill_auto_chain
`);
  writeFixture(root, DEFAULT_FILES.workflowAutoChainRules, `
pub(in crate::handlers::knowledge::workflow) fn parse_auto_chain_trigger() {}
pub(in crate::handlers::knowledge::workflow) fn evaluate_auto_trigger_safety_rules() {}
pub(in crate::handlers::knowledge::workflow) fn render_safety_rule_results() {}
pub(in crate::handlers::knowledge::workflow) fn inner_result_is_error() {}
pub(in crate::handlers::knowledge::workflow) fn chain_id_already_in_sidecar() {}
pub(in crate::handlers::knowledge::workflow) struct SafetyRuleContext {}
const AUTO_TRIGGER_DEFAULT_MIN_EVIDENCE: usize = 1;
const SAFETY_RULE_INNER_DISTILL_OK: &str = "inner_distill_succeeded";
EvidenceOutcome::Present;
`);
  writeFixture(root, DEFAULT_FILES.workflowAutoSonnet, `
mod policy;
pub(super) use policy::*;
pub(super) fn validate_auto_sonnet_args() {}
pub(super) fn auto_sonnet_requested() {}
pub(super) async fn maybe_apply_auto_sonnet() {}
pub(super) async fn maybe_apply_auto_sonnet_no_trigger() {}
`);
  writeFixture(root, DEFAULT_FILES.workflowAutoSonnetPolicy, `
use super::*;
pub(in crate::handlers::knowledge::workflow) enum AutoSonnetPolicy {}
pub(in crate::handlers::knowledge::workflow) fn parse_auto_sonnet_policy() {}
pub(in crate::handlers::knowledge::workflow) async fn maybe_apply_auto_sonnet_policy() {}
const AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR: &str = "safe_after_rules";
// review_required=true
`);
  writeFixture(root, DEFAULT_FILES.workflowMethodology, `
mod extract;
mod io;
mod source;
mod types;
mod yaml;
pub(in crate::handlers::knowledge::workflow) use self::extract::*;
pub(in crate::handlers::knowledge::workflow) use self::io::*;
pub(in crate::handlers::knowledge::workflow) use self::source::*;
pub(in crate::handlers::knowledge::workflow) use self::types::*;
pub(in crate::handlers::knowledge::workflow) use self::yaml::*;`);
  writeFixture(root, DEFAULT_FILES.workflowMethodologyTypes, `
pub(in crate::handlers::knowledge::workflow) struct MethodologyStep {}
pub(in crate::handlers::knowledge::workflow) struct MethodologyForm {}
pub(in crate::handlers::knowledge::workflow) struct MethodologyLifted {}
pub(in crate::handlers::knowledge::workflow) struct GeneratedMeta {}
pub(in crate::handlers::knowledge::workflow) enum CompiledFlowError { MissingArgs }`);
  writeFixture(root, DEFAULT_FILES.workflowMethodologySource, `
pub(in crate::handlers::knowledge::workflow) fn resolve_methodology_path() {}
pub(in crate::handlers::knowledge::workflow) fn validate_methodology_source() {}
pub(in crate::handlers::knowledge::workflow) fn source_hash() {}
pub(in crate::handlers::knowledge::workflow) fn derive_flow_id() {}
pub(in crate::handlers::knowledge::workflow) fn resolve_compiled_flow() {}`);
  writeFixture(root, DEFAULT_FILES.workflowMethodologyExtract, `
pub(in crate::handlers::knowledge::workflow) fn extract_steps() {}
pub(in crate::handlers::knowledge::workflow) fn extract_steps_with_lines() {}
pub(in crate::handlers::knowledge::workflow) fn extract_methodology_lifted() {}
pub(in crate::handlers::knowledge::workflow) fn match_form_keyword() {}
pub(in crate::handlers::knowledge::workflow) fn parse_optional_form_id() {}
pub(in crate::handlers::knowledge::workflow) fn phase_id_for_step() {}`);
  writeFixture(root, DEFAULT_FILES.workflowMethodologyYaml, `
pub(in crate::handlers::knowledge::workflow) fn build_generated_yaml() {}
pub(in crate::handlers::knowledge::workflow) fn build_manual_review_prompt() {}
fn build_methodology_metadata_yaml() {}`);
  writeFixture(root, DEFAULT_FILES.workflowMethodologyIo, `
static GENERATED_YAML_TEMP_SEQ: usize = 0;
pub(in crate::handlers::knowledge::workflow) fn unique_generated_yaml_temp_path() {}
pub(in crate::handlers::knowledge::workflow) fn atomic_write() {}`);
  writeFixture(root, DEFAULT_FILES.workflowReviewResolution, `
pub(super) const WORKFLOW_REVIEW_ACTIONS: &[&str] = &["compile"];
pub(super) const WORKFLOW_REVIEW_VERSION: i32 = 1;
pub(super) async fn action_resolve_review() {
  ResolutionOutcome::RequestChanges;
}
pub(crate) enum WorkflowSubscriberOutcome {
  MethodologyReceipt,
}
pub(crate) async fn handle_review_resolved_event() {
  WorkflowSubscriberOutcome::MethodologyReceipt;
}`);
  writeFixture(root, DEFAULT_FILES.mcpWorkflow, `
manager action — see Lisp implemented-surface mission_workflow
"distill" "compile_methodology" "run_methodology" "resolve_review"
"distill_mode" &["dry_run", "sonnet"]
"compile_mode" &["dry_run", "deterministic"]
"write_file" enriched V3 workflow artifact carrying :workflow_id/:source_plans/:match_rules/:steps/:status plus :body workflow_sexp; compile_methodology has no Workflow DB row and stamps :workflow_id with the deterministic generated flow_id, packing source_kind=\\"methodology\\" / compiler / compiler_version / source_hash / flow_id / source_path / generated_at into :match_rules
"run_methodology" artifact_only_no_workflow_row workflow_record_execution(success=true,cost_usd?)
"overwrite_file" "review_gate_policy" "review_automation_policy"
.missiond/v3/runtime/plans/<plan_id>.evidence.json
"auto_sonnet_policy" &["off", "safe_after_rules", "dry_run"]
Lisp 源: intent-flow.lisp`);
  writeFixture(root, DEFAULT_FILES.projectSsotConvergence, `
(workflow project-ssot-convergence
  :workflow_id project-ssot-convergence
  :status active
  :source_plans [fixture]
  :steps [s1b s2 s8 s8c s9]
  :inputs [project-id project-root canonical-intent existing-code dirty-baseline context-pack-path? acceptance]
  :core
    ((step s1b :id choose-write-strategy
       :logic "prefer overlay+manifest mode")
     (step s2 :id draft-l1-index
       :logic "if present and large, add an M6 overlay rather than replacing the whole file")
     (step s8 :id verify-and-report
       :logic "run scoped diff checks for owned paths")
     (step s8c :id m6-depth-handoff
       :logic ".missiond/workflows/project-m6-depth.lisp")
     (step s9 :id worker-stall-recovery
       :logic "if a ClaudeCode worker stalls after intermediate narration such as 'let me write' without file changes, reduce the shard to an atomic overlay/manifest patch"))
  :risk-gates
    ((gate g9 :rule "Dirty worktree SSOT convergence commits must stage explicit .missiond paths only")
     (gate g10 :rule "Large existing intent files should use M6 overlay+manifest")
     (gate g14 :rule "M6 production-readiness claims require domain model audit"))
  :completion
    ((criterion c5 :rule "Dirty-baseline handling is explicit")))`);
  writeFixture(root, DEFAULT_FILES.projectM6Depth, `
(workflow project-m6-depth
  :workflow_id project-m6-depth
  :status active
  :source_plans [auth-m6-depth project-ssot-convergence v3-runtime-ssot]
  :steps
    ((step s1 :id review-question :logic "ask architecture review")
     (step s2 :id evidence-plan :logic "questions hypotheses evidence_needed")
     (step s3 :id investigation :logic "Findings / Evidence / Recommendations / Verification")
     (step s4 :id synthesis :logic "findings")
     (step s5 :id design-proposal :logic "design_options")
     (step s6 :id domain-model-audit :logic "read model")
     (step s7 :id target-architecture-draft :logic "write lisp")
     (step s8 :id authority-chain-check :logic "tenant -> application -> product -> product_user -> product_user_group")
     (step s9 :id compatibility-ledger :logic "legacy bridge")
     (step s10 :id runtime-registration-check :logic "Runtime registration of new business objects does not require rebuild or redeploy")
     (step s11 :id event-contract-check :logic "producer outbox adapter sink retry ack")
     (step s12 :id hot-path-wiring-check :logic "Critical contracts must be hot-path wired")
     (step s13 :id regression-matrix :logic "old and new behavior")
     (step s14 :id exact-shards :logic "accepted_shards")
     (step s15 :id implementation :logic "worker shards")
     (step s16 :id verification :logic "report"))
  :context-pack-artifacts [questions hypotheses evidence_needed findings design_options accepted_shards]
  :risk-gates
    ((gate g1 :rule "No destructive DB migration")
     (gate g2 :rule "No production deploy, DNS mutation, or secret mutation")
     (gate g3 :rule "exact-shard-ready=true"))
  :completion
    ((criterion c1 :rule "Domain model and authority chain are explicit")))`);
  writeFixture(root, DEFAULT_FILES.multiProjectM6Wave, `
(workflow multi-project-m6-wave
  :workflow_id multi-project-m6-wave
  :status active
  :steps
    ((step s1 :id select-wave :logic "select")
     (step s2 :id review-question :logic "ask")
     (step s3 :id evidence-plan :logic "questions hypotheses evidence_needed")
     (step s4 :id investigation :logic "Findings / Evidence / Recommendations / Verification")
     (step s5 :id synthesis :logic "findings")
     (step s6 :id design-proposal :logic "design_options")
     (step s7 :id exact-shards :logic "accepted_shards")
     (step s8 :id implementation :logic "implement")
     (step s9 :id verification :logic "check-project-maturity --min-level M6"))
  :context-pack-artifacts [questions hypotheses evidence_needed findings design_options accepted_shards]
  :risk-gates ((gate g1 :rule "exact-shard-ready=true")))`);
  return root;
}

function writeFixture(root, rel, text) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text.trimStart());
}

main();
