#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-project-registry-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 project-registry Lisp/code isomorphism contract:
  - mission_project stays a thin public facade.
  - project registry actions are split into registry/context/survey/vault modules.
  - intent discovery and default universe import manifest project from V3.
  - ProjectRegistry::resolve keeps longest-prefix semantics.
  - resolve_target_project_root keeps explicit project, cwd, fallback, and no-signal behavior.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  runtimeConfig: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  facade: 'crates/missiond-daemon/src/handlers/knowledge/project.rs',
  registry: 'crates/missiond-daemon/src/handlers/knowledge/project/registry.rs',
  context: 'crates/missiond-daemon/src/handlers/knowledge/project/context.rs',
  universe: 'crates/missiond-daemon/src/handlers/knowledge/project/universe.rs',
  survey: 'crates/missiond-daemon/src/handlers/knowledge/project/survey.rs',
  vault: 'crates/missiond-daemon/src/handlers/knowledge/project/vault.rs',
  coreProject: 'crates/missiond-core/src/types/project.rs',
  rootResolver: 'crates/missiond-daemon/src/slot_orchestrator/project_root.rs',
  mcp: 'crates/missiond-mcp/src/tools/knowledge/project.rs',
  maturityChecker: 'scripts/check-project-maturity.mjs',
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
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'project-registry',
    '(v2-item project-registry',
    ':status runtime-projected',
    '(project-registry-policy',
    ':intent-path-candidates [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]',
    ':default-universe-manifest "/Users/jinchen/Projects/universe.intent.lisp"',
    '(surface project-registry',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
    'crates/missiond-daemon/src/handlers/knowledge/project.rs',
    'crates/missiond-daemon/src/handlers/knowledge/project/registry.rs',
    'crates/missiond-daemon/src/handlers/knowledge/project/context.rs',
    'crates/missiond-daemon/src/handlers/knowledge/project/survey.rs',
    'crates/missiond-daemon/src/handlers/knowledge/project/vault.rs',
    'crates/missiond-core/src/types/project.rs',
    'crates/missiond-daemon/src/slot_orchestrator/project_root.rs',
    'crates/missiond-mcp/src/tools/knowledge/project.rs',
    'scripts/check-v3-project-registry-isomorphism.mjs',
    'scripts/check-project-maturity.mjs',
    'project.rs is the thin mission_project facade',
    'ProjectRegistryRuntimeConfig loads V3 project-registry-policy',
	    'ProjectRegistry::resolve owns longest-prefix project lookup',
	    'resolve_target_project_root owns project-root spawn cwd policy',
	    '(project-maturity-model',
	    ':schema "missiond.project-maturity-model.v1"',
	    ':v3-alias M10',
	    '(level M6 :name ssot-closure',
	    '(level M7 :name runtime-projected',
	    '(level M8 :name event-driven',
	    '(level M9 :name worker-operational',
	    '(level M10 :name v3-runtime-ssot',
	    '(project-maturity-registry',
	    ':schema "missiond.project-maturity-registry.v1"',
	    ':common-m6-to-v3-gap [runtime-projection event-bus commit-backfill worker-operational final-convergence]',
	    'scripts/check-project-maturity.mjs --min-level M6',
	    'scripts/check-project-maturity.mjs --min-level M10',
	    'It resolves the MissionD blueprint from the checker script directory',
	    '(maturity :id missiond :current M10 :target M10',
	    '(maturity :id auth :current M9 :target M10',
	    '(project-blueprint-registry',
	    ':id jarvis-forge',
	    ':backend ".missiond/backend/forge-backend-blueprint.lisp"',
	    ':frontend ".missiond/frontend/forge-ui-blueprint.lisp"',
	    ':id deploy-center',
	    ':root "/Users/jinchen/Projects/xiaojinpro-backend/services/deploy-center"',
	    ':id xiaojinpro-backend',
	    ':root "/Users/jinchen/Projects/xiaojinpro-backend"',
	    ':id deploy-agent',
	    ':aliases [xjp-deploy-agent]',
	    ':root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/apps/xjp-deploy-agent"',
	    ':id auth',
	    ':root "/Users/jinchen/Projects/xiaojinpro-backend/services/auth"',
	    ':id router',
	    ':root "/Users/jinchen/Projects/xiaojinpro-backend/services/router"',
	    ':id payments',
	    ':root "/Users/jinchen/Projects/xiaojinpro-backend/services/payments"',
	    ':id asr',
	    ':root "/Users/jinchen/Projects/xiaojinpro-backend/services/asr"',
	    ':id timeline',
	    ':root "/Users/jinchen/Projects/xiaojinpro-backend/services/timeline"',
	    ':id pcea',
	    ':root "/Users/jinchen/Downloads/PCEA develop"',
	    ':backend ".missiond/backend/pcea-backend-blueprint.lisp"',
	    ':frontend ".missiond/frontend/pcea-frontend-blueprint.lisp"',
	    ':id xjp-deploy-center',
	    ':root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center"',
	    ':capability deploy-ops',
	    '(service-runtime-universe',
	    ':schema "missiond.service-runtime-universe.v1"',
	    ':id auth',
	    ':public-base-url "https://auth.xiaojinpro.com"',
	    ':deployment (:substrate kubernetes :namespace production :deployment "xjp-auth-center"',
	    ':dns-provider cloudflare',
	    ':event-ingest (:endpoint "/webhooks/auth-event" :domain system :event ExternalServiceEvent',
	    ':source auth-audit-events',
	    'mission_project(action=universe)',
	    'node scripts/check-v3-project-registry-isomorphism.mjs',
	  ]);

  requireAll(diagnostics, files.runtimeConfig, sources.runtimeConfig, [
    'ProjectRegistryRuntimeConfig',
    'DEFAULT_PROJECT_UNIVERSE_MANIFEST',
    'DEFAULT_PROJECT_INTENT_PATH_CANDIDATES',
    '.missiond/intent.lisp',
    '.jarvis/intent.lisp',
    'intent.lisp',
    'parse_project_registry_policy',
    'project-registry-policy',
    'intent-path-candidates',
    'default-universe-manifest',
    'env_or_default_universe_manifest',
    'nearest_missiond_root',
    'UNIVERSE_MANIFEST',
  ]);

  requireAll(diagnostics, files.facade, sources.facade, [
    'mod context;',
    'mod registry;',
    'mod survey;',
    'mod universe;',
    'mod vault;',
    '"list" => registry::handle_list(state).await',
    '"get" => registry::handle_get(state, args).await',
    '"set_active" => registry::handle_set_active(state, args).await',
    '"sync" => registry::handle_sync(state).await',
    '"init" => registry::handle_init(state, args).await',
    '"context" => context::handle_context(state, args).await',
    '"memories" => context::handle_memories(state, args).await',
    '"universe" => universe::handle_universe(args).await',
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
    'ProjectRegistryRuntimeConfig::load_for_current_dir',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'env_or_default_universe_manifest',
    'discover_intent_path',
    'intent_path_candidates',
    'expand_tilde_path',
    'reload_project_registry',
    'scan_lisp_files',
    'github_url_for_path',
    'backfill_project_id',
    'ProjectRegistry::new(projects)',
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

  requireAll(diagnostics, files.universe, sources.universe, [
    'handle_universe',
    'service-runtime-universe',
    'extract_forms(&block, "(service ")',
    'extract_forms(&block, "(capability ")',
    'publicBaseUrl',
    'dnsProvider',
    'opsCapability',
    'sourceEvidence',
    'locate_v3_blueprint',
  ]);

  requireAll(diagnostics, files.survey, sources.survey, [
    'handle_survey',
    'std::process::Command::new("forge")',
    'cmd.arg("survey")',
    'cmd.arg("--level")',
    'cmd.arg("--check")',
    'cmd.arg("--dry-run")',
    'ProjectRegistryRuntimeConfig::load_for_current_dir',
    'V3_BLUEPRINT_CONFIG_ERROR',
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
    '"universe"',
    '"vault_sync"',
    '"import_universe"',
    '"survey"',
  ]);

  requireAll(diagnostics, files.maturityChecker, sources.maturityChecker, [
    'fileURLToPath(import.meta.url)',
    'DEFAULT_REPO_ROOT',
    'opts.dryFixture ? buildFixture() : DEFAULT_REPO_ROOT',
    '--evidence-only',
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
  (v2-convergence-map
    (v2-item project-registry
      :status runtime-projected))
	  (project-registry-policy
	    :intent-path-candidates [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
	    :default-universe-manifest "/Users/jinchen/Projects/universe.intent.lisp")
	  (project-maturity-model
	    :schema "missiond.project-maturity-model.v1"
	    :gate "scripts/check-project-maturity.mjs --min-level M6 and scripts/check-project-maturity.mjs --min-level M10"
	    :note "It resolves the MissionD blueprint from the checker script directory"
	    :v3-alias M10
	    :levels ((level M6 :name ssot-closure)
	             (level M7 :name runtime-projected)
	             (level M8 :name event-driven)
	             (level M9 :name worker-operational)
	             (level M10 :name v3-runtime-ssot)))
	  (project-maturity-registry
	    :schema "missiond.project-maturity-registry.v1"
	    :common-m6-to-v3-gap [runtime-projection event-bus commit-backfill worker-operational final-convergence]
	    (maturity :id missiond :current M10 :target M10)
	    (maturity :id auth :current M9 :target M10))
	  (project-blueprint-registry
	    (project :id jarvis-forge :root "/Users/jinchen/Projects/jarvis-forge" :backend ".missiond/backend/forge-backend-blueprint.lisp" :frontend ".missiond/frontend/forge-ui-blueprint.lisp")
	    (project :id xiaojinpro-backend :root "/Users/jinchen/Projects/xiaojinpro-backend")
	    (project :id deploy-center :root "/Users/jinchen/Projects/xiaojinpro-backend/services/deploy-center" :capability deploy-ops)
	    (project :id deploy-agent :aliases [xjp-deploy-agent] :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/apps/xjp-deploy-agent")
	    (project :id auth :root "/Users/jinchen/Projects/xiaojinpro-backend/services/auth")
	    (project :id router :root "/Users/jinchen/Projects/xiaojinpro-backend/services/router")
	    (project :id payments :root "/Users/jinchen/Projects/xiaojinpro-backend/services/payments")
	    (project :id asr :root "/Users/jinchen/Projects/xiaojinpro-backend/services/asr")
	    (project :id timeline :root "/Users/jinchen/Projects/xiaojinpro-backend/services/timeline")
	    (project :id pcea :root "/Users/jinchen/Downloads/PCEA develop" :backend ".missiond/backend/pcea-backend-blueprint.lisp" :frontend ".missiond/frontend/pcea-frontend-blueprint.lisp")
	    (project :id xjp-deploy-center :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center" :capability deploy-ops))
    (service-runtime-universe
      :schema "missiond.service-runtime-universe.v1"
      (service :id auth :public-base-url "https://auth.xiaojinpro.com" :dns-provider cloudflare :deployment (:substrate kubernetes :namespace production :deployment "xjp-auth-center") :event-ingest (:endpoint "/webhooks/auth-event" :domain system :event ExternalServiceEvent :source auth-audit-events))
      ;; mission_project(action=universe)
      )
	  (implementation-map
    (surface project-registry
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/registry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/universe.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/survey.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/vault.rs"
             "crates/missiond-core/src/types/project.rs"
	             "crates/missiond-daemon/src/slot_orchestrator/project_root.rs"
	             "crates/missiond-mcp/src/tools/knowledge/project.rs"
	             "scripts/check-v3-project-registry-isomorphism.mjs"
	             "scripts/check-project-maturity.mjs"]
      :note "project.rs is the thin mission_project facade. ProjectRegistryRuntimeConfig loads V3 project-registry-policy. ProjectRegistry::resolve owns longest-prefix project lookup. resolve_target_project_root owns project-root spawn cwd policy."))
  (compression-contract
    :checks ["node scripts/check-v3-project-registry-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.runtimeConfig, `
ProjectRegistryRuntimeConfig DEFAULT_PROJECT_UNIVERSE_MANIFEST DEFAULT_PROJECT_INTENT_PATH_CANDIDATES
.missiond/intent.lisp .jarvis/intent.lisp intent.lisp
parse_project_registry_policy project-registry-policy intent-path-candidates default-universe-manifest
env_or_default_universe_manifest nearest_missiond_root UNIVERSE_MANIFEST
`);

  writeFixture(root, DEFAULT_FILES.facade, `
mod context;
mod registry;
mod survey;
mod universe;
mod vault;
"list" => registry::handle_list(state).await
"get" => registry::handle_get(state, args).await
"set_active" => registry::handle_set_active(state, args).await
"sync" => registry::handle_sync(state).await
"init" => registry::handle_init(state, args).await
"context" => context::handle_context(state, args).await
"memories" => context::handle_memories(state, args).await
"universe" => universe::handle_universe(args).await
"survey" => survey::handle_survey(state, args).await
"vault_sync" => vault::handle_vault_sync(state, args).await
"import_universe" => registry::handle_import_universe(state, args).await
Unknown project action
`);

  writeFixture(root, DEFAULT_FILES.registry, `
handle_list handle_get handle_set_active handle_sync handle_init handle_import_universe
ProjectRegistryRuntimeConfig::load_for_current_dir V3_BLUEPRINT_CONFIG_ERROR
env_or_default_universe_manifest discover_intent_path intent_path_candidates expand_tilde_path
reload_project_registry scan_lisp_files github_url_for_path
backfill_project_id ProjectRegistry::new(projects)
`);

  writeFixture(root, DEFAULT_FILES.context, `
handle_context handle_memories build_intent_summary build_github_info
conversation_stats_by_project recent_conversations_by_project kb_stats_by_project
build_slots_info project_memory::list_memories project_memory::read_memory_index
`);

  writeFixture(root, DEFAULT_FILES.universe, `
handle_universe service-runtime-universe
extract_forms(&block, "(service ")
extract_forms(&block, "(capability ")
publicBaseUrl dnsProvider opsCapability sourceEvidence locate_v3_blueprint
`);

  writeFixture(root, DEFAULT_FILES.survey, `
handle_survey
std::process::Command::new("forge")
cmd.arg("survey")
cmd.arg("--level")
cmd.arg("--check")
cmd.arg("--dry-run")
ProjectRegistryRuntimeConfig::load_for_current_dir
V3_BLUEPRINT_CONFIG_ERROR
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
"list" "get" "set_active" "sync" "init" "context" "memories" "universe" "vault_sync" "import_universe" "survey"
`);

  writeFixture(root, DEFAULT_FILES.maturityChecker, `
fileURLToPath(import.meta.url)
DEFAULT_REPO_ROOT
opts.dryFixture ? buildFixture() : DEFAULT_REPO_ROOT
--evidence-only
`);

  return root;
}

function writeFixture(root, rel, content) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content);
}

main();
