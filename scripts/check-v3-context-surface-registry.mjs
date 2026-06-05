#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-context-surface-registry.mjs [--json]

Checks the V3 context-surface registry:
  - MissionD declares every missiond.*context*.v* schema in one registry.
  - mission_context_gather remains the canonical runtime evidence aggregator.
  - boot/capsule/sidecar/bootstrap contexts are classified as derived or fallback surfaces.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  memoryShard: '.missiond/v3/shards/memory-knowledge-runtime.lisp',
  implementationMap: '.missiond/v3/shards/implementation/knowledge-surfaces.lisp',
  codexAppBootstrap: 'scripts/mission-context-pack.mjs',
  contextGather: 'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
  contextCapsule: 'crates/missiond-daemon/src/handlers/knowledge/context_capsule.rs',
  taskDelegate: 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
  masterControl: 'crates/missiond-daemon/src/engine/master_control.rs',
  aggregate: 'scripts/check-v3-code-isomorphism-complete.mjs',
};

const SCHEMA_RE = /missiond\.[A-Za-z0-9_.-]*context[A-Za-z0-9_.-]*\.v[0-9]+/g;

function main() {
  const args = parseArgs(process.argv.slice(2));
  const repoRoot = process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  const result = {
    ok: diagnostics.length === 0,
    diagnostics,
  };
  if (args.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 context-surface registry check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 context-surface registry check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false };
  for (const arg of argv) {
    if (arg === '--json') opts.json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }
  return opts;
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    try {
      sources[key] =
        key === 'blueprint'
          ? readBlueprintWithEvidenceSidecars(root, rel)
          : fs.readFileSync(path.join(root, rel), 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  const registryText = extractRegistryBlock(sources.memoryShard);
  if (!registryText) {
    diagnostics.push({
      file: files.memoryShard,
      message: 'missing (context-surface-registry ...) block',
    });
    return diagnostics;
  }

  requireAll(diagnostics, files.memoryShard, registryText, [
    'missiond.context-surface-registry.v1',
    ':canonical-runtime-builder mission_context_gather',
    ':canonical-startup-surface mission_context_boot',
    ':canonical-planning-ledger context-pack',
    '(class planning-ledger',
    '(class runtime-gather',
    '(class derived-binding',
    '(class startup-contract',
    '(class worker-sidecar',
    '(class support-context',
    '(surface runtime-context-gather',
    ':authority canonical-runtime-context-builder',
    '(surface codex-app-bootstrap-hints',
    ':authority fallback-hints-only',
    '(surface master-control-context-pack',
    '(surface swarm-context-pack',
    'mission_context_gather is the only runtime surface allowed to aggregate evidence lanes',
    'New missiond.*context*.v* schemas must be declared in context-surface-registry',
  ]);

  requireAll(diagnostics, files.implementationMap, sources.implementationMap, [
    '(surface context-surface-registry',
    ':status "code-aligned"',
    'context-schema-dedupe',
    'scripts/check-v3-context-surface-registry.mjs',
    'mission_context_gather is the only runtime evidence-lane aggregator',
  ]);

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'context-surface-registry',
    'node scripts/check-v3-context-surface-registry.mjs',
  ]);

  requireAll(diagnostics, files.aggregate, sources.aggregate, [
    'context-surface-registry',
    'scripts/check-v3-context-surface-registry.mjs',
  ]);

  requireAll(diagnostics, files.codexAppBootstrap, sources.codexAppBootstrap, [
    "schema: 'missiond.codex-app-context-pack.v1'",
    "context_authority: 'fallback-hints-only'",
    '--live',
    'mission_context_boot',
    'mission_context_gather',
    'deterministic Codex App bootstrap fallback',
  ]);

  requireAll(diagnostics, files.contextGather, sources.contextGather, [
    '"schema": "missiond.context-gather.v1"',
    '"schema": "missiond.context-gather-artifact.v1"',
    'context_pack_artifact_payload',
    'materialize_context_pack_file',
    'super::context_capsule::generate_lisp_capsule',
  ]);

  requireAll(diagnostics, files.contextCapsule, sources.contextCapsule, [
    'missiond.context-capsule.v1',
    'generate_lisp_capsule',
    'context_gather_payload',
  ]);

  requireAll(diagnostics, files.taskDelegate, sources.taskDelegate, [
    'missiond.swarm-context-pack.v1',
    'render_swarm_context_pack',
    'grounding_context_id',
    'normalize_context_pack_path_for_worker',
  ]);

  requireAll(diagnostics, files.masterControl, sources.masterControl, [
    'missiond.master-control-context-pack.v1',
    'render_master_context_pack',
    'mission-context-gather',
  ]);

  const discovered = discoverContextSchemas(root);
  for (const schema of discovered) {
    if (!registryText.includes(schema)) {
      diagnostics.push({
        file: files.memoryShard,
        message: `context schema is used but not registered: ${schema}`,
      });
    }
  }

  return diagnostics;
}

function extractRegistryBlock(source) {
  const start = source.indexOf('(context-surface-registry');
  if (start < 0) return '';
  const end = source.indexOf('\n  (evidence-lane-policy', start);
  return source.slice(start, end > start ? end : undefined);
}

function discoverContextSchemas(root) {
  const roots = [
    '.missiond/v3',
    '.missiond/frontend',
    'scripts',
    'crates',
    'packages',
  ];
  const schemas = new Set();
  for (const relRoot of roots) {
    const absRoot = path.join(root, relRoot);
    for (const file of walk(absRoot)) {
      let text = '';
      try {
        text = fs.readFileSync(file, 'utf8');
      } catch {
        continue;
      }
      for (const match of text.matchAll(SCHEMA_RE)) schemas.add(match[0]);
    }
  }
  return [...schemas].sort();
}

function* walk(dir) {
  let entries = [];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (shouldSkipDir(entry.name, full)) continue;
      yield* walk(full);
    } else if (entry.isFile() && shouldReadFile(full)) {
      yield full;
    }
  }
}

function shouldSkipDir(name, fullPath) {
  if (['.git', 'node_modules', 'target', '.next', 'dist', 'build'].includes(name)) return true;
  return fullPath.includes('/.missiond/v3/runtime/') || fullPath.includes('/.missiond/tasks/');
}

function shouldReadFile(file) {
  return /\.(lisp|rs|mjs|js|ts|tsx|json|md)$/.test(file);
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing ${needle}` });
    }
  }
}

main();
