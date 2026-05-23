#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
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
    const trackedRuntimeArtifacts = gitTrackedRuntimeArtifacts(repo);
    if (trackedRuntimeArtifacts.error) {
      diagnostics.push({
        file: '.missiond/v3/runtime',
        message: trackedRuntimeArtifacts.error,
      });
    } else if (trackedRuntimeArtifacts.files.length > 0) {
      diagnostics.push({
        file: '.missiond/v3/runtime',
        message: `.missiond/v3/runtime must remain untracked cold evidence; tracked files: ${trackedRuntimeArtifacts.files.slice(0, 10).join(', ')}`,
      });
    }
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

function gitTrackedRuntimeArtifacts(repo) {
  const proc = spawnSync('git', ['ls-files', '.missiond/v3/runtime'], {
    cwd: repo,
    encoding: 'utf8',
    timeout: 30_000,
    maxBuffer: 1024 * 1024,
  });
  if (proc.status !== 0 || proc.error) {
    return {
      files: [],
      error: proc.error?.message ?? proc.stderr?.trim() ?? 'git ls-files failed',
    };
  }
  return {
    files: proc.stdout.split('\n').map((line) => line.trim()).filter(Boolean),
    error: null,
  };
}

main();
