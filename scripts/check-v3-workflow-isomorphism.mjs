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
  workflowMethodology: 'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs',
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
    'crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs',
    'node scripts/check-v3-workflow-isomorphism.mjs',
  ]);

  const workflowSurface = `${sources.workflowHandler}\n${sources.workflowArtifacts}\n${sources.workflowMethodology}`;
  const workflowSurfaceLabel = `${files.workflowHandler} + ${files.workflowArtifacts} + ${files.workflowMethodology}`;
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
  ]);

  requireAll(diagnostics, files.workflowMethodology, sources.workflowMethodology, [
    'pub(super) fn extract_methodology_lifted',
    'pub(super) fn build_generated_yaml',
    'pub(super) fn resolve_compiled_flow',
    'pub(super) struct GeneratedMeta',
  ]);

  requireAll(diagnostics, files.workflowArtifacts, sources.workflowArtifacts, [
    'pub(super) fn extract_workflow_file_args',
    'pub(super) async fn maybe_write_workflow_artifact',
    'pub(super) fn render_workflow_artifact_sexp',
    'pub(super) fn build_methodology_match_rules',
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
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs"]
      :note "distill dry_run emits workflow-draft Lisp; sonnet distiller requires JSON workflow_sexp + object match_rules; distill persist+write_file writes an enriched V3 workflow artifact with :body workflow_sexp; compile_methodology reads methodology Lisp from .missiond/workflows/<name>.lisp; persist+write_file path now also projects the methodology compile through render_workflow_artifact_sexp with :match_rules carrying source_kind=methodology / compiler / compiler_version / source_hash / flow_id, :status compiled, :body methodology lisp body, instead of canonicalizing the raw methodology source — no Workflow DB row is introduced; ArtifactKind::Workflow; auto_sonnet_policy={off|safe_after_rules|dry_run}"))
  (compression-contract
    :checks ["node scripts/check-v3-workflow-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.workflowHandler, `
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
mod tests {
  fn methodology_compile_renders_v3_workflow_artifact_not_raw_source() {}
  fn methodology_compile_review_required_status_when_no_steps() {}
  fn build_methodology_match_rules_includes_flow_id_and_source_hash() {}
}`);
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
  writeFixture(root, DEFAULT_FILES.workflowMethodology, `
pub(super) struct GeneratedMeta {}
pub(super) fn extract_methodology_lifted() {}
pub(super) fn build_generated_yaml() {}
pub(super) fn resolve_compiled_flow() {}`);
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
