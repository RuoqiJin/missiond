#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-memory-kb-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 memory-kb convergence contract. This surface is still
:status "designed", but the checker pins the first physical Rust split:
kb.rs stays the facade, while kb/args.rs, kb/quality.rs, kb/compact.rs,
and kb/conflicts.rs own the corresponding V3 function boundaries.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  kbFacade: 'crates/missiond-daemon/src/handlers/knowledge/kb.rs',
  kbArgs: 'crates/missiond-daemon/src/handlers/knowledge/kb/args.rs',
  kbQuality: 'crates/missiond-daemon/src/handlers/knowledge/kb/quality.rs',
  kbCompact: 'crates/missiond-daemon/src/handlers/knowledge/kb/compact.rs',
  kbConflicts: 'crates/missiond-daemon/src/handlers/knowledge/kb/conflicts.rs',
  kbQuery: 'crates/missiond-daemon/src/handlers/knowledge/kb/query.rs',
  kbMutate: 'crates/missiond-daemon/src/handlers/knowledge/kb/mutate.rs',
  kbImport: 'crates/missiond-daemon/src/handlers/knowledge/kb/import.rs',
  mcpKb: 'crates/missiond-mcp/src/tools/knowledge/kb.rs',
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
    console.log('v3 memory-kb Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 memory-kb Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
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
      sources[key] = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    '(surface memory-kb',
    ':status "designed"',
    'crates/missiond-daemon/src/handlers/knowledge/kb.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/args.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/quality.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/compact.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/conflicts.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/query.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/mutate.rs',
    'crates/missiond-daemon/src/handlers/knowledge/kb/import.rs',
    'scripts/check-v3-memory-kb-isomorphism.mjs',
    'kb.rs remains the memory-kb facade',
    'kb/args.rs owns unified KB argument ingress',
    'kb/quality.rs owns content-quality rejection',
    'kb/compact.rs owns rule-based KB compaction',
    'kb/conflicts.rs owns semantic conflict detection',
    'kb/query.rs owns get/list JSON egress',
    'kb/mutate.rs owns forget/update/project mutation side effects',
    'kb/import.rs owns servers_yaml import projection',
    'node scripts/check-v3-memory-kb-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.kbFacade, sources.kbFacade, [
    'mod args;',
    'mod compact;',
    'mod conflicts;',
    'mod import;',
    'mod mutate;',
    'mod quality;',
    'mod query;',
    'use args::{',
    'use compact::handle_kb_compact;',
    'use conflicts::detect_kb_conflicts;',
    'use import::handle_kb_import;',
    'handle_kb_batch_forget',
    'handle_kb_batch_set_project',
    'handle_kb_forget',
    'handle_kb_update',
    'use quality::check_content_quality;',
    'use query::{handle_kb_get, handle_kb_list};',
    'pub(crate) async fn handle',
    '"mission_kb_query"',
    '"mission_kb_mutate"',
    '"mission_kb_ops"',
    '"mission_kb_remember"',
  ]);

  requireAll(diagnostics, files.kbArgs, sources.kbArgs, [
    'pub(super) struct KBRememberArgs',
    'pub(super) struct KBKeyArgs',
    'pub(super) struct KBUpdateArgs',
    'pub(super) struct KBSearchArgs',
    'pub(super) struct KBListArgs',
    'pub(super) struct KBImportArgs',
    'pub(super) struct KBDiscoverArgs',
    'pub(super) struct KBGCArgs',
    'lenient::option_i64',
    'fn default_list_limit()',
  ]);

  requireAll(diagnostics, files.kbQuality, sources.kbQuality, [
    'pub(super) fn check_content_quality',
    'architecture:summary',
    'summary 过长',
    'summary 为空',
    'test write',
    'batch-',
    'stack trace',
    'RUST_BACKTRACE',
    'detail 过长',
  ]);

  requireAll(diagnostics, files.kbCompact, sources.kbCompact, [
    'pub(super) async fn handle_kb_compact',
    'dryRun',
    'kb_list(None)',
    'low_confidence',
    'stale_state',
    'stale_ops',
    'stale_debug',
    'stale_bugfix',
    'low_value_fact',
    'expired_scratchpad',
    'kb_batch_forget',
  ]);

  requireAll(diagnostics, files.kbConflicts, sources.kbConflicts, [
    'pub(super) async fn detect_kb_conflicts',
    'CONFLICT_SIM_THRESHOLD',
    'embedding_service',
    'cosine_similarity',
    'text_jaccard',
    'category_prefix',
    'conflicts.truncate(5)',
  ]);

  requireAll(diagnostics, files.kbQuery, sources.kbQuery, [
    'pub(super) async fn handle_kb_get',
    'pub(super) async fn handle_kb_list',
    'KBKeyArgs',
    'KBListArgs',
    'kb_get(&key)',
    'kb_list_paginated',
    '"compact": true',
    'Key not found',
  ]);

  requireAll(diagnostics, files.kbMutate, sources.kbMutate, [
    'pub(super) async fn handle_kb_forget',
    'pub(super) async fn handle_kb_batch_forget',
    'pub(super) async fn handle_kb_batch_set_project',
    'pub(super) async fn handle_kb_update',
    'check_content_quality',
    'kb_get_id_by_key',
    'kb_batch_forget',
    'kb_update',
    'EmbeddingTask::ProcessKBEntry',
    'KBBatchMutated',
  ]);

  requireAll(diagnostics, files.kbImport, sources.kbImport, [
    'pub(super) async fn handle_kb_import',
    'KBImportArgs',
    'servers_yaml',
    'default_mission_home',
    'InfraConfig::load',
    'KBRememberInput',
    'Unsupported import format',
  ]);

  requireAll(diagnostics, files.mcpKb, sources.mcpKb, [
    '"mission_kb_query"',
    '"mission_kb_remember"',
    '"mission_kb_mutate"',
    '"mission_kb_ops"',
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
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-memory-kb-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (implementation-map
    (surface memory-kb
      :status "designed"
      :code ["crates/missiond-daemon/src/handlers/knowledge/kb.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/args.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/quality.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/compact.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/conflicts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/query.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/mutate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/import.rs"
             "scripts/check-v3-memory-kb-isomorphism.mjs"]
      :note "kb.rs remains the memory-kb facade; kb/args.rs owns unified KB argument ingress; kb/quality.rs owns content-quality rejection; kb/compact.rs owns rule-based KB compaction; kb/conflicts.rs owns semantic conflict detection; kb/query.rs owns get/list JSON egress; kb/mutate.rs owns forget/update/project mutation side effects; kb/import.rs owns servers_yaml import projection."))
  (compression-contract
    :checks ["node scripts/check-v3-memory-kb-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.kbFacade, `
mod args;
mod compact;
mod conflicts;
mod import;
mod mutate;
mod quality;
mod query;
use args::{KBRememberArgs};
use compact::handle_kb_compact;
use conflicts::detect_kb_conflicts;
use import::handle_kb_import;
handle_kb_batch_forget; handle_kb_batch_set_project; handle_kb_forget; handle_kb_update;
use quality::check_content_quality;
use query::{handle_kb_get, handle_kb_list};
pub(crate) async fn handle() {
  "mission_kb_query"; "mission_kb_mutate"; "mission_kb_ops"; "mission_kb_remember";
}`);
  writeFixture(root, DEFAULT_FILES.kbArgs, `
pub(super) struct KBRememberArgs;
pub(super) struct KBKeyArgs;
pub(super) struct KBUpdateArgs;
pub(super) struct KBSearchArgs;
pub(super) struct KBListArgs;
pub(super) struct KBImportArgs;
pub(super) struct KBDiscoverArgs;
pub(super) struct KBGCArgs;
lenient::option_i64;
fn default_list_limit() {}
`);
  writeFixture(root, DEFAULT_FILES.kbQuality, `
pub(super) fn check_content_quality() {
  architecture:summary; summary 过长; summary 为空; test write; batch-; stack trace; RUST_BACKTRACE; detail 过长;
}`);
  writeFixture(root, DEFAULT_FILES.kbCompact, `
pub(super) async fn handle_kb_compact() {
  dryRun; kb_list(None); low_confidence; stale_state; stale_ops; stale_debug; stale_bugfix; low_value_fact; expired_scratchpad; kb_batch_forget;
}`);
  writeFixture(root, DEFAULT_FILES.kbConflicts, `
pub(super) async fn detect_kb_conflicts() {
  CONFLICT_SIM_THRESHOLD; embedding_service; cosine_similarity; text_jaccard; category_prefix; conflicts.truncate(5);
}`);
  writeFixture(root, DEFAULT_FILES.kbQuery, `
pub(super) async fn handle_kb_get() {
  KBKeyArgs; kb_get(&key); Key not found;
}
pub(super) async fn handle_kb_list() {
  KBListArgs; kb_list_paginated(); "compact": true;
}`);
  writeFixture(root, DEFAULT_FILES.kbMutate, `
pub(super) async fn handle_kb_forget() { kb_get_id_by_key(); KBBatchMutated; }
pub(super) async fn handle_kb_batch_forget() { kb_batch_forget(); KBBatchMutated; }
pub(super) async fn handle_kb_batch_set_project() { kb_update(); }
pub(super) async fn handle_kb_update() {
  check_content_quality(); kb_update(); EmbeddingTask::ProcessKBEntry; KBBatchMutated;
}`);
  writeFixture(root, DEFAULT_FILES.kbImport, `
pub(super) async fn handle_kb_import() {
  KBImportArgs; servers_yaml; default_mission_home(); InfraConfig::load(); KBRememberInput; Unsupported import format;
}`);
  writeFixture(root, DEFAULT_FILES.mcpKb, `
"mission_kb_query"; "mission_kb_remember"; "mission_kb_mutate"; "mission_kb_ops";
`);
  return root;
}

function writeFixture(root, rel, content) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content.trimStart());
}

main();
