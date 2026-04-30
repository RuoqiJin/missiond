#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-workflow-isomorphism.mjs [--json] [--dry-fixture]

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
  workflowAutoSonnet: 'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet.rs',
  workflowDistill: 'crates/missiond-daemon/src/handlers/knowledge/workflow/distill.rs',
  workflowMethodology: 'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs',
  workflowReviewResolution: 'crates/missiond-daemon/src/handlers/knowledge/workflow/review_resolution.rs',
  workflowTests: 'crates/missiond-daemon/src/handlers/knowledge/workflow/tests.rs',
  mcpWorkflow: 'crates/missiond-mcp/src/tools/knowledge/workflow.rs',
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
    console.log('v3 workflow Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(`v3 workflow Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }

  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = fs.readFileSync(abs, 'utf8');
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
    'distill persist+write_file writes an enriched V3 workflow artifact',
    ':body workflow_sexp',
    'compile_methodology reads methodology Lisp from .missiond/workflows/<name>.lisp',
    'persist+write_file path now also projects the methodology compile through render_workflow_artifact_sexp',
    'source_kind=methodology',
    ':status compiled',
    'no Workflow DB row',
    'instead of canonicalizing the raw methodology source',
    'ArtifactKind::Workflow',
    'auto_sonnet_policy={off|safe_after_rules|dry_run}',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/artifacts.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain/recorder.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/distill.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/review_resolution.rs',
    'workflow/distill.rs owns DistillMode',
    'crates/missiond-daemon/src/handlers/knowledge/workflow/tests.rs',
    'workflow/review_resolution.rs owns resolve_review',
    'WorkflowSubscriberOutcome',
    'node scripts/check-v3-workflow-isomorphism.mjs',
  ]);

  const workflowSurface = `${sources.workflowHandler}\n${sources.workflowArtifacts}\n${sources.workflowAutoChain}\n${sources.workflowAutoChainRecorder}\n${sources.workflowAutoSonnet}\n${sources.workflowDistill}\n${sources.workflowMethodology}\n${sources.workflowReviewResolution}\n${sources.workflowTests}`;
  const workflowSurfaceLabel = `${files.workflowHandler} + ${files.workflowArtifacts} + ${files.workflowAutoChain} + ${files.workflowAutoChainRecorder} + ${files.workflowAutoSonnet} + ${files.workflowDistill} + ${files.workflowMethodology} + ${files.workflowReviewResolution} + ${files.workflowTests}`;
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
    'mod distill;',
    'use distill::action_distill;',
  ]);

  requireAll(diagnostics, files.workflowDistill, sources.workflowDistill, [
    'pub(super) enum DistillMode',
    'pub(super) fn parse_distill_mode',
    'pub(super) async fn action_distill',
    'async fn action_distill_dry_run',
    'pub(super) async fn action_distill_sonnet',
    'build_distiller_prompt(plan, name, &match_hint, &evidence_value)',
    'pub(super) enum EvidenceOutcome',
    'pub(super) fn evidence_sidecar_path',
    'pub(super) fn read_evidence_sidecar',
    'pub(super) fn validate_workflow_sexp',
    'pub(super) fn paren_balanced_ignoring_strings',
  ]);

  requireAll(diagnostics, files.workflowMethodology, sources.workflowMethodology, [
    'pub(super) fn extract_methodology_lifted',
    'pub(super) fn build_generated_yaml',
    'pub(super) fn resolve_compiled_flow',
    'pub(super) struct GeneratedMeta',
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
    'pub(super) use recorder::{',
    'pub(super) async fn maybe_apply_distill_chain_layers',
    'pub(super) enum AutoChainTrigger',
    'pub(super) fn parse_auto_chain_trigger',
    'pub(super) fn evaluate_auto_trigger_safety_rules',
    'pub(super) fn render_safety_rule_results',
    'AUTO_TRIGGER_DEFAULT_MIN_EVIDENCE',
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

  requireAll(diagnostics, files.workflowAutoSonnet, sources.workflowAutoSonnet, [
    'pub(super) fn validate_auto_sonnet_args',
    'pub(super) fn auto_sonnet_requested',
    'pub(super) async fn maybe_apply_auto_sonnet',
    'pub(super) async fn maybe_apply_auto_sonnet_no_trigger',
    'pub(super) enum AutoSonnetPolicy',
    'pub(super) fn parse_auto_sonnet_policy',
    'pub(super) async fn maybe_apply_auto_sonnet_policy',
    'AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR',
    'review_required=true',
  ]);

  requireAll(diagnostics, files.workflowTests, sources.workflowTests, [
    'use super::*;',
    'methodology_compile_renders_v3_workflow_artifact_not_raw_source',
    'methodology_compile_review_required_status_when_no_steps',
    'build_methodology_match_rules_includes_flow_id_and_source_hash',
  ]);

  requireAll(diagnostics, files.mcpWorkflow, sources.mcpWorkflow, [
    'manager action — see Lisp implemented-surface mission_workflow',
    '"distill"',
    '"compile_methodology"',
    '"run_methodology"',
    '"resolve_review"',
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

function requireText(diagnostics, file, source, needle) {
  if (!source.includes(needle)) {
    diagnostics.push({ file, message: `missing required contract text: ${needle}` });
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-workflow-isomorphism-'));
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
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/distill.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/review_resolution.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/tests.rs"]
      :note "workflow/distill.rs owns DistillMode and action_distill. distill dry_run emits workflow-draft Lisp; sonnet distiller requires JSON workflow_sexp + object match_rules; distill persist+write_file writes an enriched V3 workflow artifact with :body workflow_sexp; compile_methodology reads methodology Lisp from .missiond/workflows/<name>.lisp; persist+write_file path now also projects the methodology compile through render_workflow_artifact_sexp with :match_rules carrying source_kind=methodology / compiler / compiler_version / source_hash / flow_id, :status compiled, :body methodology lisp body, instead of canonicalizing the raw methodology source — no Workflow DB row is introduced; ArtifactKind::Workflow; workflow/auto_chain/recorder.rs owns the wave-19 explicit recorder; auto_sonnet_policy={off|safe_after_rules|dry_run}; workflow/review_resolution.rs owns resolve_review and WorkflowSubscriberOutcome"))
  (compression-contract
    :checks ["node scripts/check-v3-workflow-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.workflowHandler, `
mod distill;
use distill::action_distill;
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
enum CompileMode {}
fn parse_compile_mode() {}
async fn action_compile_methodology() {}
fn action_compile_dry_run() {}
async fn action_compile_deterministic() {
  validate_methodology_source(content);
  extract_steps_with_lines(content);
  let methodology_match_rules = build_methodology_match_rules(&meta);
  render_workflow_artifact_sexp(&meta.flow_id, &[], &methodology_match_rules, "compiled", content);
}
async fn action_run_methodology() {}
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
fn build_methodology_match_rules_includes_flow_id_and_source_hash() {}`);
  writeFixture(root, DEFAULT_FILES.workflowArtifacts, `
pub(super) fn extract_workflow_file_args() {}
pub(super) async fn maybe_write_workflow_artifact() {}
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
pub(super) use recorder::{maybe_apply_auto_chain, AUTO_CHAIN_EVIDENCE_SOURCE};
pub(super) async fn maybe_apply_distill_chain_layers() {}
pub(super) enum AutoChainTrigger {}
pub(super) fn parse_auto_chain_trigger() {}
pub(super) fn evaluate_auto_trigger_safety_rules() {}
pub(super) fn render_safety_rule_results() {}
const AUTO_TRIGGER_DEFAULT_MIN_EVIDENCE: usize = 1;
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
  writeFixture(root, DEFAULT_FILES.workflowAutoSonnet, `
pub(super) fn validate_auto_sonnet_args() {}
pub(super) fn auto_sonnet_requested() {}
pub(super) async fn maybe_apply_auto_sonnet() {}
pub(super) async fn maybe_apply_auto_sonnet_no_trigger() {}
pub(super) enum AutoSonnetPolicy {}
pub(super) fn parse_auto_sonnet_policy() {}
pub(super) async fn maybe_apply_auto_sonnet_policy() {}
const AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR: &str = "safe_after_rules";
// review_required=true
`);
  writeFixture(root, DEFAULT_FILES.workflowMethodology, `
pub(super) struct GeneratedMeta {}
pub(super) fn extract_methodology_lifted() {}
pub(super) fn build_generated_yaml() {}
pub(super) fn resolve_compiled_flow() {}`);
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
"overwrite_file" "review_gate_policy" "review_automation_policy"
"auto_sonnet_policy" &["off", "safe_after_rules", "dry_run"]
Lisp 源: intent-flow.lisp`);
  return root;
}

function writeFixture(root, rel, text) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text.trimStart());
}

main();
