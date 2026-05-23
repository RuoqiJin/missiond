#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import {
  BASELINE_AGENT_ENTRY_IDS,
  PRIMARY_TOOL_FAMILIES,
  validateCompiledAgentSlices,
  validateCompiledProjectAgentNavigation,
} from './lib/v3_agent_slices.mjs';

const SEMANTIC_IR = '.missiond/v3/runtime/compiled/compiled-semantic-ir.json';
const AGENT_SLICES = '.missiond/v3/runtime/compiled/compiled-agent-slices.json';
const PROJECT_AGENT_NAVIGATION = '.missiond/v3/runtime/compiled/compiled-project-agent-navigation.json';
const MCP_TOOL_DIRECTORY = 'crates/missiond-mcp/src/tools/comm/tool_directory.rs';
const DAEMON_TOOL_DIRECTORY = 'crates/missiond-daemon/src/handlers/comm/tool_directory.rs';
const CONTEXT_SLICE_TOOL = 'crates/missiond-mcp/src/tools/knowledge/shared_memory.rs';
const SHARED_MEMORY_RUNTIME = 'crates/missiond-daemon/src/engine/shared_memory.rs';

function main() {
  const json = process.argv.includes('--json');
  const repo = process.cwd();
  const diagnostics = [];
  const semantic = readJson(path.join(repo, SEMANTIC_IR), SEMANTIC_IR, diagnostics);
  const compiled = readJson(path.join(repo, AGENT_SLICES), AGENT_SLICES, diagnostics);
  const projectNavigation = readJson(path.join(repo, PROJECT_AGENT_NAVIGATION), PROJECT_AGENT_NAVIGATION, diagnostics);

  const facts = Array.isArray(semantic?.payload?.facts) ? semantic.payload.facts : [];
  const rawEntries = facts.filter((fact) => fact?.kind === 'agent_entry');
  if (rawEntries.length === 0) {
    diagnostics.push(diag(SEMANTIC_IR, 'compiled semantic IR must include agent_entry facts'));
  }
  if (!facts.some((fact) => fact?.kind === 'agent_navigation_policy' && fact.id === 'closure')) {
    diagnostics.push(diag(SEMANTIC_IR, 'compiled semantic IR must include agent_navigation_policy closure fact'));
  }

  diagnostics.push(...validateCompiledAgentSlices(compiled));
  diagnostics.push(...validateCompiledProjectAgentNavigation(projectNavigation));
  const entries = Array.isArray(compiled?.payload?.entries) ? compiled.payload.entries : [];
  const factsByKind = {
    surface: mapByKind(facts, 'surface'),
    function: mapByKind(facts, 'function'),
    artifact_contract: mapByKind(facts, 'artifact_contract'),
    runtime_policy: mapByKind(facts, 'runtime_policy'),
  };

  for (const id of BASELINE_AGENT_ENTRY_IDS) {
    if (!rawEntries.some((entry) => entry.id === id)) {
      diagnostics.push(diag(SEMANTIC_IR, `semantic IR missing baseline agent_entry ${id}`));
    }
  }

  for (const entry of rawEntries) {
    if (!PRIMARY_TOOL_FAMILIES.includes(entry.primary_family)) {
      diagnostics.push(diag(sourceFile(entry), `unknown primary_family ${entry.primary_family ?? '<missing>'} on ${entry.id}`));
    }
    checkRefs(diagnostics, entry, 'surfaces', factsByKind.surface, 'surface');
    checkRefs(diagnostics, entry, 'functions', factsByKind.function, 'function');
    checkRefs(diagnostics, entry, 'artifact_contracts', factsByKind.artifact_contract, 'artifact_contract');
    checkRefs(diagnostics, entry, 'runtime_policies', factsByKind.runtime_policy, 'runtime_policy');
    for (const command of stringArray(entry.checks)) {
      checkCommandExists(repo, diagnostics, entry, command);
    }
  }

  for (const entry of entries) {
    for (const file of stringArray(entry.readFirst)) {
      if (!fs.existsSync(path.join(repo, file))) {
        diagnostics.push(diag(AGENT_SLICES, `entry ${entry.id} readFirst file does not exist: ${file}`));
      }
    }
  }

  requireNeedles(repo, diagnostics, MCP_TOOL_DIRECTORY, [
    '"guide"',
    'entry_id',
    'surface',
  ]);
  requireNeedles(repo, diagnostics, DAEMON_TOOL_DIRECTORY, [
    '"guide"',
    'guide_tool_directory',
    'missiond.tool-directory-guide.v1',
    'AGENT_SLICES_UNAVAILABLE',
  ]);
  requireNeedles(repo, diagnostics, CONTEXT_SLICE_TOOL, [
    '"intent"',
    '"entry_id"',
    '"entryId"',
  ]);
  requireNeedles(repo, diagnostics, SHARED_MEMORY_RUNTIME, [
    'compiled-agent-slices.json',
    'agentEntry',
  ]);

  const result = {
    ok: diagnostics.length === 0,
    semantic_agent_entries: rawEntries.length,
    compiled_entries: entries.length,
    project_navigation_projects: Array.isArray(projectNavigation?.payload?.projects) ? projectNavigation.payload.projects.length : 0,
    baseline_entries: BASELINE_AGENT_ENTRY_IDS.length,
    diagnostics,
  };
  if (json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log('v3 agent entry slices check OK');
  else for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
  process.exit(result.ok ? 0 : 1);
}

function checkRefs(diagnostics, entry, field, map, label) {
  for (const id of stringArray(entry[field])) {
    if (!map.has(id)) diagnostics.push(diag(sourceFile(entry), `agent entry ${entry.id} references unknown ${label} ${id}`));
  }
}

function checkCommandExists(repo, diagnostics, entry, command) {
  const match = command.match(/^node\s+(scripts\/[^ ]+)/);
  if (match && !fs.existsSync(path.join(repo, match[1]))) {
    diagnostics.push(diag(sourceFile(entry), `agent entry ${entry.id} check command file is missing: ${match[1]}`));
  }
  const pnpmDir = command.match(/^pnpm\s+--dir\s+([^ ]+)/);
  if (pnpmDir && !fs.existsSync(path.join(repo, pnpmDir[1], 'package.json'))) {
    diagnostics.push(diag(sourceFile(entry), `agent entry ${entry.id} pnpm package is missing: ${pnpmDir[1]}`));
  }
}

function requireNeedles(repo, diagnostics, rel, needles) {
  let source = '';
  try {
    source = fs.readFileSync(path.join(repo, rel), 'utf8');
  } catch (err) {
    diagnostics.push(diag(rel, `cannot read: ${err.message}`));
    return;
  }
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push(diag(rel, `missing required text: ${needle}`));
  }
}

function readJson(file, rel, diagnostics) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (err) {
    diagnostics.push(diag(rel, `cannot read JSON: ${err.message}`));
    return null;
  }
}

function mapByKind(facts, kind) {
  return new Map(
    facts
      .filter((fact) => fact?.kind === kind && typeof fact.id === 'string')
      .map((fact) => [fact.id, fact]),
  );
}

function stringArray(value) {
  return Array.isArray(value) ? value.filter((item) => typeof item === 'string' && item.length > 0) : [];
}

function sourceFile(entry) {
  return entry?.source?.source_file ?? '.missiond/v3/shards/agent-navigation.lisp';
}

function diag(file, message) {
  return { file, line: 1, column: 1, code: 'V3_AGENT_ENTRY_SLICES', message };
}

main();
