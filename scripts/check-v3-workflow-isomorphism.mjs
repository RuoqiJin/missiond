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
  - file-first workflow writes, review gates, and auto-Sonnet policy are tied to the same surface.
  - MCP schema exposes the same distill/compile/run/review knobs.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  workflowHandler: 'crates/missiond-daemon/src/handlers/knowledge/workflow.rs',
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
    ':status "code-aligned-partial"',
    'distill dry_run emits workflow-draft Lisp',
    'sonnet distiller requires JSON workflow_sexp + object match_rules',
    'compile_methodology reads methodology Lisp from .missiond/workflows/<name>.lisp',
    'ArtifactKind::Workflow',
    'auto_sonnet_policy={off|safe_after_rules|dry_run}',
    'node scripts/check-v3-workflow-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.workflowHandler, sources.workflowHandler, [
    'enum DistillMode',
    'fn parse_distill_mode',
    'Some("dry_run") => Ok(DistillMode::DryRun)',
    'Some("sonnet") => Ok(DistillMode::Sonnet)',
    'async fn action_distill_dry_run',
    '"(workflow-draft\\n  :name',
    '"compiled_sexp_preview": preview_sexp',
    '.workflow_insert(name, &preview_sexp, &json!({}), Some(plan.id))',
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
    'parse_review_gate_policy(args)',
    'apply_compile_review_gates(',
    'fn parse_auto_sonnet_policy',
    '"safe_after_rules"',
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
      :status "code-aligned-partial"
      :note "distill dry_run emits workflow-draft Lisp; sonnet distiller requires JSON workflow_sexp + object match_rules; compile_methodology reads methodology Lisp from .missiond/workflows/<name>.lisp; ArtifactKind::Workflow; auto_sonnet_policy={off|safe_after_rules|dry_run}"))
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
}
async fn action_run_methodology() {}
fn extract_workflow_file_args() {}
ArtifactKind::Workflow;
attempt_artifact_write();
parse_review_gate_policy(args);
apply_compile_review_gates();
fn parse_auto_sonnet_policy() { "safe_after_rules"; }`);
  writeFixture(root, DEFAULT_FILES.mcpWorkflow, `
manager action — see Lisp implemented-surface mission_workflow
"distill" "compile_methodology" "run_methodology" "resolve_review"
"distill_mode" &["dry_run", "sonnet"]
"compile_mode" &["dry_run", "deterministic"]
"write_file" "overwrite_file" "review_gate_policy" "review_automation_policy"
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
