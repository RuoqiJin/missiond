#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  gitignore: '.gitignore',
  migration: 'crates/missiond-core/migrations/20260523000000_runtime_artifacts.sql',
  sharedMemory: 'crates/missiond-daemon/src/engine/shared_memory.rs',
};

function main() {
  const json = process.argv.includes('--json');
  const repo = process.cwd();
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(FILES)) {
    try {
      sources[key] = key === 'blueprint'
        ? readBlueprintWithEvidenceSidecars(repo, rel)
        : fs.readFileSync(path.join(repo, rel), 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length === 0) {
    requireAll('blueprint', [
      'runtime_artifacts',
      '.missiond/v3/runtime/jarvis-smoke/*.json',
      'indexed in Postgres runtime_artifacts',
      'canonical task/plan evidence is indexed without automatic deletion',
    ], sources, diagnostics);
    requireAll('gitignore', [
      '.missiond/v3/runtime/jarvis-smoke/*.json',
      '.missiond/v3/runtime/jarvis-smoke/*.lisp',
      '.missiond/v3/runtime/jarvis-smoke/*.report.*',
    ], sources, diagnostics);
    requireAll('migration', [
      'CREATE TABLE IF NOT EXISTS runtime_artifacts',
      'hash TEXT NOT NULL',
      'path TEXT NOT NULL',
      'kind TEXT NOT NULL',
      'source_surface TEXT',
      'expires_at TIMESTAMPTZ',
      'UNIQUE(path, hash)',
    ], sources, diagnostics);
    requireAll('sharedMemory', [
      'runtime_artifact_index',
      'runtime_artifact_list',
      'runtime_artifact_prune',
      'runtimeArtifacts',
      'runtime_artifacts_for_scope',
      'runtime_artifact_expires_at',
      'runtime_artifact_row_json',
    ], sources, diagnostics);
  }

  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log('v3 runtime artifact catalog check OK');
  else for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
  process.exit(result.ok ? 0 : 1);
}

function requireAll(key, needles, sources, diagnostics) {
  for (const needle of needles) {
    if (!sources[key].includes(needle)) {
      diagnostics.push({
        file: FILES[key],
        line: 1,
        column: 1,
        code: 'RUNTIME_ARTIFACT_CATALOG_MISSING',
        message: `missing runtime artifact catalog anchor: ${needle}`,
      });
    }
  }
}

main();
