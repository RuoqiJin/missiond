#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-project-registry-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 project-registry Lisp/code isomorphism contract:
  - mission_project stays a thin public facade.
  - project registry actions are split into registry/context/survey/vault modules.
  - ProjectRegistry::resolve keeps longest-prefix semantics.
  - resolve_target_project_root keeps explicit project, cwd, fallback, and no-signal behavior.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  facade: 'crates/missiond-daemon/src/handlers/knowledge/project.rs',
  registry: 'crates/missiond-daemon/src/handlers/knowledge/project/registry.rs',
  context: 'crates/missiond-daemon/src/handlers/knowledge/project/context.rs',
  survey: 'crates/missiond-daemon/src/handlers/knowledge/project/survey.rs',
  vault: 'crates/missiond-daemon/src/handlers/knowledge/project/vault.rs',
  coreProject: 'crates/missiond-core/src/types/project.rs',
  rootResolver: 'crates/missiond-daemon/src/slot_orchestrator/project_root.rs',
  mcp: 'crates/missiond-mcp/src/tools/knowledge/project.rs',
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
    console.log('v3 project-registry Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 project-registry Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
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
    'project-registry',
    '(surface project-registry',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/handlers/knowledge/project.rs',
    'crates/missiond-daemon/src/handlers/knowledge/project/registry.rs',
    'crates/missiond-daemon/src/handlers/knowledge/project/context.rs',
    'crates/missiond-daemon/src/handlers/knowledge/project/survey.rs',
    'crates/missiond-daemon/src/handlers/knowledge/project/vault.rs',
    'crates/missiond-core/src/types/project.rs',
    'crates/missiond-daemon/src/slot_orchestrator/project_root.rs',
    'crates/missiond-mcp/src/tools/knowledge/project.rs',
    'scripts/check-v3-project-registry-isomorphism.mjs',
    'project.rs is the thin mission_project facade',
    'ProjectRegistry::resolve owns longest-prefix project lookup',
    'resolve_target_project_root owns project-root spawn cwd policy',
    'node scripts/check-v3-project-registry-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.facade, sources.facade, [
    'mod context;',
    'mod registry;',
    'mod survey;',
    'mod vault;',
    '"list" => registry::handle_list(state).await',
    '"get" => registry::handle_get(state, args).await',
    '"set_active" => registry::handle_set_active(state, args).await',
    '"sync" => registry::handle_sync(state).await',
    '"init" => registry::handle_init(state, args).await',
    '"context" => context::handle_context(state, args).await',
    '"memories" => context::handle_memories(state, args).await',
    '"survey" => survey::handle_survey(state, args).await',
    '"vault_sync" => vault::handle_vault_sync(state, args).await',
    '"import_universe" => registry::handle_import_universe(state, args).await',
    'Unknown project action',
  ]);

  requireAll(diagnostics, files.registry, sources.registry, [
    'handle_list',
    'handle_get',
    'handle_set_active',
    'handle_sync',
    'handle_init',
    'handle_import_universe',
    'discover_intent_path',
    'reload_project_registry',
    'scan_lisp_files',
    'github_url_for_path',
    'backfill_project_id',
    'ProjectRegistry::new(projects)',
    '.missiond/intent.lisp',
    '.jarvis/intent.lisp',
    'intent.lisp',
  ]);

  requireAll(diagnostics, files.context, sources.context, [
    'handle_context',
    'handle_memories',
    'build_intent_summary',
    'build_github_info',
    'conversation_stats_by_project',
    'recent_conversations_by_project',
    'kb_stats_by_project',
    'build_slots_info',
    'project_memory::list_memories',
    'project_memory::read_memory_index',
  ]);

  requireAll(diagnostics, files.survey, sources.survey, [
    'handle_survey',
    'std::process::Command::new("forge")',
    'cmd.arg("survey")',
    'cmd.arg("--level")',
    'cmd.arg("--check")',
    'cmd.arg("--dry-run")',
    'discover_intent_path',
    'truncate_chars',
  ]);

  requireAll(diagnostics, files.vault, sources.vault, [
    'handle_vault_sync',
    '.missiond/vault',
    '_meta.json',
    'synced_at',
    'vault_path',
    'kind = "reference".to_string()',
  ]);

  requireAll(diagnostics, files.coreProject, sources.coreProject, [
    'pub struct ProjectConfig',
    'pub struct ProjectRegistry',
    'path_index.sort_by',
    'cwd.starts_with(prefix.as_str())',
    'pub fn resolve(&self, cwd: &str) -> Option<&str>',
    'pub fn exclusive_slots(&self, project_id: &str) -> Vec<String>',
  ]);

  requireAll(diagnostics, files.rootResolver, sources.rootResolver, [
    'resolve_target_project_root',
    'explicit_project_id',
    'explicit_cwd',
    'fallback_project_id',
    'ProjectRootResolution',
    'requested_cwd',
    'CwdLongestPrefix',
    'FallbackProjectId',
    'NoSignal',
    'sub_path_or_none',
    'explicit_project_id_resolves_to_root',
    'cwd_subdir_resolves_to_root_and_preserves_requested',
    'fallback_project_id_used_when_no_explicit',
  ]);

  requireAll(diagnostics, files.mcp, sources.mcp, [
    'ToolDefinition::new',
    '"mission_project"',
    '"list"',
    '"get"',
    '"set_active"',
    '"sync"',
    '"init"',
    '"context"',
    '"memories"',
    '"vault_sync"',
    '"import_universe"',
    '"survey"',
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
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-project-registry-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (implementation-map
    (surface project-registry
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/knowledge/project.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/registry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/survey.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/vault.rs"
             "crates/missiond-core/src/types/project.rs"
             "crates/missiond-daemon/src/slot_orchestrator/project_root.rs"
             "crates/missiond-mcp/src/tools/knowledge/project.rs"
             "scripts/check-v3-project-registry-isomorphism.mjs"]
      :note "project.rs is the thin mission_project facade. ProjectRegistry::resolve owns longest-prefix project lookup. resolve_target_project_root owns project-root spawn cwd policy."))
  (compression-contract
    :checks ["node scripts/check-v3-project-registry-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.facade, `
mod context;
mod registry;
mod survey;
mod vault;
"list" => registry::handle_list(state).await
"get" => registry::handle_get(state, args).await
"set_active" => registry::handle_set_active(state, args).await
"sync" => registry::handle_sync(state).await
"init" => registry::handle_init(state, args).await
"context" => context::handle_context(state, args).await
"memories" => context::handle_memories(state, args).await
"survey" => survey::handle_survey(state, args).await
"vault_sync" => vault::handle_vault_sync(state, args).await
"import_universe" => registry::handle_import_universe(state, args).await
Unknown project action
`);

  writeFixture(root, DEFAULT_FILES.registry, `
handle_list handle_get handle_set_active handle_sync handle_init handle_import_universe
discover_intent_path reload_project_registry scan_lisp_files github_url_for_path
backfill_project_id ProjectRegistry::new(projects)
.missiond/intent.lisp .jarvis/intent.lisp intent.lisp
`);

  writeFixture(root, DEFAULT_FILES.context, `
handle_context handle_memories build_intent_summary build_github_info
conversation_stats_by_project recent_conversations_by_project kb_stats_by_project
build_slots_info project_memory::list_memories project_memory::read_memory_index
`);

  writeFixture(root, DEFAULT_FILES.survey, `
handle_survey
std::process::Command::new("forge")
cmd.arg("survey")
cmd.arg("--level")
cmd.arg("--check")
cmd.arg("--dry-run")
discover_intent_path
truncate_chars
`);

  writeFixture(root, DEFAULT_FILES.vault, `
handle_vault_sync
.missiond/vault
_meta.json
synced_at
vault_path
kind = "reference".to_string()
`);

  writeFixture(root, DEFAULT_FILES.coreProject, `
pub struct ProjectConfig
pub struct ProjectRegistry
path_index.sort_by
cwd.starts_with(prefix.as_str())
pub fn resolve(&self, cwd: &str) -> Option<&str>
pub fn exclusive_slots(&self, project_id: &str) -> Vec<String>
`);

  writeFixture(root, DEFAULT_FILES.rootResolver, `
resolve_target_project_root explicit_project_id explicit_cwd fallback_project_id
ProjectRootResolution requested_cwd CwdLongestPrefix FallbackProjectId NoSignal sub_path_or_none
explicit_project_id_resolves_to_root
cwd_subdir_resolves_to_root_and_preserves_requested
fallback_project_id_used_when_no_explicit
`);

  writeFixture(root, DEFAULT_FILES.mcp, `
ToolDefinition::new
"mission_project"
"list" "get" "set_active" "sync" "init" "context" "memories" "vault_sync" "import_universe" "survey"
`);

  return root;
}

function writeFixture(root, rel, content) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content);
}

main();
