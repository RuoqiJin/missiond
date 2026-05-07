#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-skill-runtime-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 skill-runtime Lisp/code isomorphism contract:
  - mission_skill_* public tools stay routed through a thin facade.
  - query/context/mutate/exec behavior lives in separate Rust modules.
  - workflow execution stays isolated from query and mutation paths.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  facade: 'crates/missiond-daemon/src/handlers/knowledge/skill.rs',
  query: 'crates/missiond-daemon/src/handlers/knowledge/skill/query.rs',
  context: 'crates/missiond-daemon/src/handlers/knowledge/skill/context.rs',
  mutate: 'crates/missiond-daemon/src/handlers/knowledge/skill/mutate.rs',
  exec: 'crates/missiond-daemon/src/handlers/knowledge/skill/exec.rs',
  mcp: 'crates/missiond-mcp/src/tools/knowledge/skill.rs',
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
    console.log('v3 skill-runtime Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 skill-runtime Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
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
    'skill-runtime',
    '(surface skill-runtime',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/handlers/knowledge/skill.rs',
    'crates/missiond-daemon/src/handlers/knowledge/skill/query.rs',
    'crates/missiond-daemon/src/handlers/knowledge/skill/context.rs',
    'crates/missiond-daemon/src/handlers/knowledge/skill/mutate.rs',
    'crates/missiond-daemon/src/handlers/knowledge/skill/exec.rs',
    'crates/missiond-mcp/src/tools/knowledge/skill.rs',
    'scripts/check-v3-skill-runtime-isomorphism.mjs',
    'skill.rs is the thin mission_skill facade',
    'skill/query.rs owns list/search/topics/actions/stats',
    'composite registry/topic/action/embedding/execution statistics',
    'project-skill-link readiness',
    'skill/context.rs owns context build/resolve',
    'skill/mutate.rs owns upsert/record/render/rollback',
    'skill/exec.rs owns mission_skill_exec',
    'node scripts/check-v3-skill-runtime-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.facade, sources.facade, [
    'mod context;',
    'mod exec;',
    'mod mutate;',
    'mod query;',
    'route_skill_query',
    'route_skill_context',
    'route_skill_mutate',
    '"mission_skill_query"',
    '"mission_skill_context"',
    '"mission_skill_mutate"',
    '"mission_skill_exec" => exec::handle_exec(state, args).await',
    '"mission_skill_list" => query::handle_list(state).await',
    '"mission_context_build" => context::handle_build(state, args).await',
    '"mission_skill_upsert" => mutate::handle_upsert(state, args).await',
    'Unknown skill tool',
  ]);

  requireAll(diagnostics, files.query, sources.query, [
    'handle_list',
    'handle_search',
    'handle_topics',
    'handle_actions',
    'handle_stats',
    'skill_search_fts',
    'rrf_score',
    'skill_topic_hit',
    'parse_workflow_blocks',
    'skill_execution_stats',
    'missiond.skill-stats.v1',
    'loadedSkills',
    'skill_topic_list',
    'skill_embedding_cache',
    'actionCount',
    'projectSkillLinks',
  ]);

  requireAll(diagnostics, files.context, sources.context, [
    'handle_build',
    'handle_resolve',
    'build_context',
    'kb_search',
    'SkillRequires',
    'include_board',
    'list_board_tasks',
    'state.infra.read',
    '"skills": skill_results',
    '"board": board_results',
  ]);

  requireAll(diagnostics, files.mutate, sources.mutate, [
    'handle_upsert',
    'handle_record',
    'handle_render',
    'handle_rollback',
    'ensure_topic_exists',
    'skill_topic_upsert',
    'skill_block_insert',
    'materialize_topic',
    'materialize_all',
    'ProcessSkillTopic',
    'skill_version_get',
    'skill_version_list',
    'ingest_skills',
  ]);

  requireAll(diagnostics, files.exec, sources.exec, [
    'handle_exec',
    'execute_workflow',
    'dry_run',
    'params',
    'Workflow execution failed',
  ]);

  requireAll(diagnostics, files.mcp, sources.mcp, [
    'ToolDefinition::new',
    '"mission_skill_query"',
    '"mission_skill_context"',
    '"mission_skill_mutate"',
    '"mission_skill_exec"',
    '"list"',
    '"search"',
    '"topics"',
    '"actions"',
    '"stats"',
    '"build"',
    '"resolve"',
    '"upsert"',
    '"record"',
    '"render"',
    '"rollback"',
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
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-skill-runtime-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (implementation-map
    (surface skill-runtime
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/knowledge/skill.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/query.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/mutate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/exec.rs"
             "crates/missiond-mcp/src/tools/knowledge/skill.rs"
             "scripts/check-v3-skill-runtime-isomorphism.mjs"]
      :note "skill.rs is the thin mission_skill facade; skill/query.rs owns list/search/topics/actions/stats, composite registry/topic/action/embedding/execution statistics, and project-skill-link readiness; skill/context.rs owns context build/resolve; skill/mutate.rs owns upsert/record/render/rollback; skill/exec.rs owns mission_skill_exec."))
  (compression-contract
    :checks ["node scripts/check-v3-skill-runtime-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.facade, `
mod context;
mod exec;
mod mutate;
mod query;
route_skill_query route_skill_context route_skill_mutate
"mission_skill_query" "mission_skill_context" "mission_skill_mutate"
"mission_skill_exec" => exec::handle_exec(state, args).await
"mission_skill_list" => query::handle_list(state).await
"mission_context_build" => context::handle_build(state, args).await
"mission_skill_upsert" => mutate::handle_upsert(state, args).await
Unknown skill tool
`);

  writeFixture(root, DEFAULT_FILES.query, `
handle_list handle_search handle_topics handle_actions handle_stats
skill_search_fts rrf_score skill_topic_hit parse_workflow_blocks skill_execution_stats
missiond.skill-stats.v1 loadedSkills skill_topic_list skill_embedding_cache actionCount projectSkillLinks
`);

  writeFixture(root, DEFAULT_FILES.context, `
handle_build handle_resolve build_context kb_search SkillRequires include_board
list_board_tasks state.infra.read
"skills": skill_results
"board": board_results
`);

  writeFixture(root, DEFAULT_FILES.mutate, `
handle_upsert handle_record handle_render handle_rollback ensure_topic_exists
skill_topic_upsert skill_block_insert materialize_topic materialize_all
ProcessSkillTopic skill_version_get skill_version_list ingest_skills
`);

  writeFixture(root, DEFAULT_FILES.exec, `
handle_exec execute_workflow dry_run params Workflow execution failed
`);

  writeFixture(root, DEFAULT_FILES.mcp, `
ToolDefinition::new
"mission_skill_query" "mission_skill_context" "mission_skill_mutate" "mission_skill_exec"
"list" "search" "topics" "actions" "stats" "build" "resolve" "upsert" "record" "render" "rollback"
`);

  return root;
}

function writeFixture(root, rel, content) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content);
}

main();
