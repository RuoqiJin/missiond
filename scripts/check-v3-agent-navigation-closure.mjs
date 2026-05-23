#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import {
  BASELINE_AGENT_ENTRY_IDS,
  validateCompiledAgentSlices,
  validateCompiledProjectAgentNavigation,
} from './lib/v3_agent_slices.mjs';

const FILES = {
  shard: '.missiond/v3/shards/agent-navigation.lisp',
  semantic: '.missiond/v3/runtime/compiled/compiled-semantic-ir.json',
  slices: '.missiond/v3/runtime/compiled/compiled-agent-slices.json',
  projectNavigation: '.missiond/v3/runtime/compiled/compiled-project-agent-navigation.json',
  mcpToolDirectory: 'crates/missiond-mcp/src/tools/comm/tool_directory.rs',
  mcpAgentNavigation: 'crates/missiond-mcp/src/tools/comm/agent_navigation.rs',
  daemonAgentNavigation: 'crates/missiond-daemon/src/handlers/comm/agent_navigation.rs',
  daemonToolDirectory: 'crates/missiond-daemon/src/handlers/comm/tool_directory.rs',
  sharedMemory: 'crates/missiond-daemon/src/engine/shared_memory.rs',
  boardBlueprint: '.missiond/frontend/board-blueprint.lisp',
  boardApi: 'packages/board/src/app/api/agent-navigation/route.ts',
  boardDashboard: 'packages/board/src/components/AgentNavigationDashboard.tsx',
  cli: 'scripts/agent-navigation.mjs',
  gitignore: '.gitignore',
  finalConvergence: '.missiond/v3/missiond-blueprint.lisp',
};

function main() {
  const json = process.argv.includes('--json');
  const diagnostics = [];
  const shard = read(FILES.shard, diagnostics);
  requireNeedles(diagnostics, FILES.shard, shard, [
    '(agent-navigation-policy closure',
    ':review-sidecar ".missiond/v3/runtime/agent-navigation-review.json"',
    ':project-navigation-artifact ".missiond/v3/runtime/compiled/compiled-project-agent-navigation.json"',
    ':project-mode read-only-registered-projects',
    'mission_agent_navigation may write only the review sidecar',
  ]);
  for (const id of BASELINE_AGENT_ENTRY_IDS) {
    if (!shard.includes(`:id ${id}`)) diagnostics.push(diag(FILES.shard, `missing baseline entry ${id}`));
  }

  const semantic = readJson(FILES.semantic, diagnostics);
  const facts = Array.isArray(semantic?.payload?.facts) ? semantic.payload.facts : [];
  if (!facts.some((fact) => fact?.kind === 'agent_navigation_policy' && fact.id === 'closure')) {
    diagnostics.push(diag(FILES.semantic, 'missing agent_navigation_policy closure fact'));
  }

  diagnostics.push(...validateCompiledAgentSlices(readJson(FILES.slices, diagnostics)));
  diagnostics.push(...validateCompiledProjectAgentNavigation(readJson(FILES.projectNavigation, diagnostics)));

  requireNeedles(diagnostics, FILES.mcpToolDirectory, read(FILES.mcpToolDirectory, diagnostics), [
    '"guide"', 'project_id', 'projectId',
  ]);
  requireNeedles(diagnostics, FILES.mcpAgentNavigation, read(FILES.mcpAgentNavigation, diagnostics), [
    'mission_agent_navigation', '"catalog"', '"review"', '"feedback"', '"suggest_entries"',
  ]);
  requireNeedles(diagnostics, FILES.daemonAgentNavigation, read(FILES.daemonAgentNavigation, diagnostics), [
    'missiond.agent-navigation.catalog.v1',
    'agent-navigation-review.json',
    'atomic_write_json',
    'suggest_entries',
  ]);
  requireNeedles(diagnostics, FILES.daemonToolDirectory, read(FILES.daemonToolDirectory, diagnostics), [
    'compiled-project-agent-navigation.json',
    'project_agent_navigation',
  ]);
  requireNeedles(diagnostics, FILES.sharedMemory, read(FILES.sharedMemory, diagnostics), [
    'compiled-project-agent-navigation.json',
    'coverageDiagnostic',
  ]);
  requireNeedles(diagnostics, FILES.boardBlueprint, read(FILES.boardBlueprint, diagnostics), [
    '(tab :id navigator :label "Navigator"',
  ]);
  requireNeedles(diagnostics, FILES.boardApi, read(FILES.boardApi, diagnostics), [
    'mission_agent_navigation',
    'mission_tool_directory',
  ]);
  requireNeedles(diagnostics, FILES.boardDashboard, read(FILES.boardDashboard, diagnostics), [
    'AgentNavigationDashboard',
    '/api/agent-navigation',
  ]);
  requireNeedles(diagnostics, FILES.cli, read(FILES.cli, diagnostics), [
    'agent-navigation',
    'mission_agent_navigation',
    'evaluate',
  ]);
  requireNeedles(diagnostics, FILES.gitignore, read(FILES.gitignore, diagnostics), [
    '.missiond/v3/runtime/agent-navigation-review.json',
  ]);
  requireNeedles(diagnostics, FILES.finalConvergence, read(FILES.finalConvergence, diagnostics), [
    'agent-navigation-quality',
    'agent-navigation-closure',
    'compiled-project-agent-navigation.json',
  ]);

  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log('v3 agent navigation closure check OK');
  else for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
  process.exit(result.ok ? 0 : 1);
}

function requireNeedles(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push(diag(file, `missing required text: ${needle}`));
  }
}

function read(rel, diagnostics) {
  try {
    return fs.readFileSync(path.resolve(rel), 'utf8');
  } catch (err) {
    diagnostics.push(diag(rel, `cannot read: ${err.message}`));
    return '';
  }
}

function readJson(rel, diagnostics) {
  try {
    return JSON.parse(read(rel, diagnostics));
  } catch (err) {
    diagnostics.push(diag(rel, `cannot parse JSON: ${err.message}`));
    return null;
  }
}

function diag(file, message) {
  return { file, line: 1, column: 1, code: 'V3_AGENT_NAVIGATION_CLOSURE', message };
}

main();
