#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-intent-alignment-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 intent-alignment Lisp/code isomorphism contract:
  - V3 blueprint declares intent-alignment.lisp as the human review artifact.
  - mission_directive compile dry_run and sonnet both produce Lisp-shaped intent artifacts.
  - persisted directive Lisp is enriched with directive_id/version refs.
  - file-first intent-alignment writes and review gates are tied to the same artifact.
  - MCP schema exposes the same compile/persist/review knobs.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  directiveHandler: 'crates/missiond-daemon/src/handlers/knowledge/directive.rs',
  directiveCompileAuthoring: 'crates/missiond-daemon/src/handlers/knowledge/directive/compile_authoring.rs',
  directiveApprovalReview: 'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review.rs',
  directiveApprovalProposer: 'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/proposer.rs',
  directiveTests: 'crates/missiond-daemon/src/handlers/knowledge/directive/tests.rs',
  mcpDirective: 'crates/missiond-mcp/src/tools/knowledge/directive.rs',
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
    console.log('v3 intent-alignment Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(`v3 intent-alignment Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
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
    '(artifact intent-alignment',
    ':path ".missiond/requests/<request_id>/intent-alignment.lisp"',
    ':compat-path ".missiond/alignment/<topic>/intent-alignment.lisp"',
    'intent-alignment files MUST carry :directive_id + :version',
    '(surface mission_directive',
    ':status "code-aligned"',
    'dry_run emits a deterministic directive-draft Lisp artifact',
    'sonnet output is accepted only when it is one balanced Lisp s-expression',
    'mission_directive sonnet compiler_model labels',
    'router-runtime-policy queued_sonnet_model',
    'ArtifactKind::IntentAlignment',
    'crates/missiond-daemon/src/handlers/knowledge/directive/compile_authoring.rs',
    'crates/missiond-daemon/src/handlers/knowledge/directive/tests.rs',
    'node scripts/check-v3-intent-alignment-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.directiveHandler, sources.directiveHandler, [
    'RouterRuntimeConfig',
    'pub(super) fn load_sonnet_compiler_model',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'queued_sonnet_model',
    'mod compile_authoring;',
    'use compile_authoring::action_compile',
    'mod tests;',
  ]);
  forbidAll(diagnostics, files.directiveHandler, sources.directiveHandler, [
    'const SONNET_COMPILER_MODEL',
    'SONNET_COMPILER_MODEL: &str = "claude-sonnet"',
  ]);

  requireAll(diagnostics, files.directiveCompileAuthoring, sources.directiveCompileAuthoring, [
    'const ALLOWED_SEXP_HEADS: &[&str]',
    '"directive-draft"',
    '"intent-alignment"',
    'pub(super) async fn action_compile',
    'COMPILER_MODE_DRY_RUN',
    'COMPILER_MODE_SONNET',
    'async fn action_compile_dry_run',
    '"(directive-draft\\n  :utterance',
    '"compiled_sexp_preview": preview_sexp',
    'async fn action_compile_sonnet',
    'validate_compiled_sexp(&raw)',
    'load_sonnet_compiler_model()',
    'compiler_model.clone()',
    '"compiled_sexp": compiled_sexp',
    'fn validate_compiled_sexp',
    'strip_fenced_code_block(raw)',
    'parens_balanced(trimmed)',
    'top_level_head(trimmed)',
    'fn enrich_persisted_directive_sexp',
    ':directive_id',
    ':version',
    'fn extract_directive_file_args',
    'ArtifactKind::IntentAlignment',
    'attempt_artifact_write(',
    'apply_compile_review_gates(',
  ]);
  forbidAll(diagnostics, files.directiveCompileAuthoring, sources.directiveCompileAuthoring, [
    'SONNET_COMPILER_MODEL',
  ]);

  requireAll(diagnostics, files.directiveApprovalReview, sources.directiveApprovalReview, [
    'load_sonnet_compiler_model',
    'build_directive_automation_ctx',
    'sonnet records the V3-projected compiler model',
  ]);
  forbidAll(diagnostics, files.directiveApprovalReview, sources.directiveApprovalReview, [
    'SONNET_COMPILER_MODEL',
    'sonnet records `claude-sonnet`',
  ]);

  requireAll(diagnostics, files.directiveApprovalProposer, sources.directiveApprovalProposer, [
    'load_sonnet_compiler_model()',
    'compiler_model.clone()',
    'DIRECTIVE_REVIEW_PROPOSER_CALLER',
  ]);
  forbidAll(diagnostics, files.directiveApprovalProposer, sources.directiveApprovalProposer, [
    'SONNET_COMPILER_MODEL',
    'Some("claude-sonnet".to_string())',
  ]);

  requireAll(diagnostics, files.directiveTests, sources.directiveTests, [
    'enrich_persisted_directive_sexp_adds_ref_before_final_paren',
    'validate_accepts_intent_alignment',
    'extract_directive_file_args_defaults_are_inert',
    'directive_resolution_envelope_accepts_canonical_approve',
  ]);

  requireAll(diagnostics, files.mcpDirective, sources.mcpDirective, [
    '默认 compiler_mode=\\"dry_run\\" 不调 LLM',
    'compiler_mode=\\"sonnet\\" 走 SonnetGateway interactive',
    'compiled_sexp 镜像写到 `<project_root>/.missiond/alignment/<topic>/intent-alignment.lisp`',
    '不实现 UI / 不等回答 / 不自动 approve',
    '"compiler_mode"',
    '"enum": ["dry_run", "sonnet"]',
    '"write_file"',
    '"topic"',
    '"directive_id"',
    '"review_decision"',
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
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-intent-alignment-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (artifact-contracts
    (artifact intent-alignment
      :path ".missiond/requests/<request_id>/intent-alignment.lisp"
      :compat-path ".missiond/alignment/<topic>/intent-alignment.lisp"
      :materialization-rule "intent-alignment files MUST carry :directive_id + :version"))
  (implementation-map
    (surface mission_directive
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/knowledge/directive/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/tests.rs"]
      :model-projection "mission_directive sonnet compiler_model labels project from router-runtime-policy queued_sonnet_model"
      :note "dry_run emits a deterministic directive-draft Lisp artifact; sonnet output is accepted only when it is one balanced Lisp s-expression; ArtifactKind::IntentAlignment"))
  (compression-contract
    :checks ["node scripts/check-v3-intent-alignment-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.directiveHandler, `
use crate::context::v3_blueprint_runtime::RouterRuntimeConfig;
pub(super) fn load_sonnet_compiler_model() {
  let router_config = RouterRuntimeConfig::load_for_current_dir().unwrap();
  let _ = "V3_BLUEPRINT_CONFIG_ERROR";
  let _ = router_config.queued_sonnet_model;
}
mod compile_authoring;
use compile_authoring::action_compile;
mod tests;`);
  writeFixture(root, DEFAULT_FILES.directiveCompileAuthoring, `
const ALLOWED_SEXP_HEADS: &[&str] = &["directive", "directive-draft", "intent-alignment"];
pub(super) async fn action_compile() {}
const COMPILER_MODE_DRY_RUN: &str = "dry_run";
const COMPILER_MODE_SONNET: &str = "sonnet";
async fn action_compile_dry_run() {
  "(directive-draft\\n  :utterance";
  "compiled_sexp_preview": preview_sexp;
}
async fn action_compile_sonnet() {
  validate_compiled_sexp(&raw);
  let compiler_model = load_sonnet_compiler_model().unwrap();
  let _ = compiler_model.clone();
  "compiled_sexp": compiled_sexp;
}
fn validate_compiled_sexp() {
  strip_fenced_code_block(raw);
  parens_balanced(trimmed);
  top_level_head(trimmed);
}
fn enrich_persisted_directive_sexp() { ":directive_id"; ":version"; }
fn extract_directive_file_args() {}
ArtifactKind::IntentAlignment;
attempt_artifact_write();
apply_compile_review_gates();`);
  writeFixture(root, DEFAULT_FILES.directiveApprovalReview, `
use super::load_sonnet_compiler_model;
fn build_directive_automation_ctx() {
  "sonnet records the V3-projected compiler model";
}`);
  writeFixture(root, DEFAULT_FILES.directiveApprovalProposer, `
const DIRECTIVE_REVIEW_PROPOSER_CALLER: &str = "directive_review_proposer";
fn proposal() {
  let compiler_model = load_sonnet_compiler_model().unwrap();
  let _ = compiler_model.clone();
}`);
  writeFixture(root, DEFAULT_FILES.directiveTests, `
fn enrich_persisted_directive_sexp_adds_ref_before_final_paren() {}
fn validate_accepts_intent_alignment() {}
fn extract_directive_file_args_defaults_are_inert() {}
fn directive_resolution_envelope_accepts_canonical_approve() {}`);
  writeFixture(root, DEFAULT_FILES.mcpDirective, `
默认 compiler_mode=\\"dry_run\\" 不调 LLM
compiler_mode=\\"sonnet\\" 走 SonnetGateway interactive
compiled_sexp 镜像写到 \`<project_root>/.missiond/alignment/<topic>/intent-alignment.lisp\`
不实现 UI / 不等回答 / 不自动 approve
"compiler_mode" "enum": ["dry_run", "sonnet"] "write_file" "topic" "directive_id" "review_decision"`);
  return root;
}

function writeFixture(root, rel, text) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text.trimStart());
}

main();
